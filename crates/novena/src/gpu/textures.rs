//! Bounded sampled images and explicit host interpretations. Provenance: 0030.
use super::{
    graphics::{instructions, mapped},
    pipelines::stage_bindings,
    uniforms::UniformStage,
    Context,
};
use crate::api::SamplerDescription;
use ash::vk;
use std::{collections::HashSet, sync::Arc};

/// One recorded slot mapped to the translator's original descriptor pair.
#[derive(Clone, Copy, Debug)]
pub struct TextureMapping {
    pub stage: u64,
    pub index: u64,
    pub target: UniformStage,
    pub set: u32,
    pub image: u32,
    pub sampler: u32,
}

/// Host choices for unresolved enums and bindings. No implicit guest mapping.
#[derive(Clone, Default)]
pub struct TextureContract {
    pub bindings: Vec<TextureMapping>,
    pub filters: Vec<(u64, vk::Filter)>,
    pub wraps: Vec<(u64, vk::SamplerAddressMode)>,
    pub compare_disabled: u64,
    /// Explicit combined-handle interpretation: handle, texture id, sampler id.
    /// Separate handles returned by this library use registration ids.
    pub combined: Vec<(u64, u32, u32)>,
}
impl TextureContract {
    pub(crate) fn validate(&self) -> Result<(), String> {
        let mut sources = HashSet::new();
        let mut targets = HashSet::new();
        for m in &self.bindings {
            if m.set > 1
                || m.image >= 256
                || m.sampler >= 256
                || m.image == m.sampler
                || !sources.insert((m.stage, m.index))
                || !targets.insert((m.target, m.set, m.image))
                || !targets.insert((m.target, m.set, m.sampler))
            {
                return Err("duplicate or invalid texture mapping".into());
            }
        }
        let mut handles = HashSet::new();
        if self.combined.iter().any(|(h, _, _)| !handles.insert(*h)) {
            return Err("duplicate combined texture handle".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Binding {
    pub stage: UniformStage,
    pub set: u32,
    pub binding: u32,
    pub ty: vk::DescriptorType,
}
impl Binding {
    pub fn lowered(self) -> u32 {
        self.set * 256 + self.binding
    }
    pub fn stage_set(self) -> u32 {
        self.stage.set() + 2
    }
}

/// Reflect the separate float 2D image/sampler interface. Other shapes fail closed.
pub(crate) fn bindings(words: &[u32], stage: UniformStage) -> Result<Vec<Binding>, String> {
    let reflected = stage_bindings(words, stage.model())?;
    let instructions = instructions(words)?;
    for &(op, args) in &instructions {
        if op == 25
            && (!matches!(args, [_, _, 1, 0, 0, 0, 1, 0])
                || !instructions.contains(&(22, &[args[1], 32])))
        {
            return Err("draw supports only non-array float 2D sampled images".into());
        }
    }
    reflected
        .into_iter()
        .filter(|b| b.descriptor_type != vk::DescriptorType::UNIFORM_BUFFER)
        .map(|b| {
            if b.set > 1
                || b.binding >= 256
                || b.count != 1
                || !matches!(
                    b.descriptor_type,
                    vk::DescriptorType::SAMPLED_IMAGE | vk::DescriptorType::SAMPLER
                )
            {
                return Err("unsupported draw texture descriptor".into());
            }
            Ok(Binding {
                stage,
                set: b.set,
                binding: b.binding,
                ty: b.descriptor_type,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SamplerKey {
    min: i32,
    mag: i32,
    wrap: [i32; 3],
}
impl SamplerKey {
    pub fn new(d: &SamplerDescription, c: &TextureContract) -> Option<Self> {
        if d.compare_mode != c.compare_disabled
            || d.max_anisotropy != 1.0
            || d.lod_bias != 0.0
            || d.lod_clamp != [0.0; 2]
            || d.border_color != [0.0; 4]
        {
            return None;
        }
        let min = mapped(&c.filters, d.min_filter)?;
        let mag = mapped(&c.filters, d.mag_filter)?;
        if ![min, mag]
            .iter()
            .all(|v| matches!(*v, vk::Filter::NEAREST | vk::Filter::LINEAR))
        {
            return None;
        }
        let mut wrap = [0; 3];
        for (i, token) in d.wrap.iter().enumerate() {
            let value = mapped(&c.wraps, *token)?;
            if !matches!(
                value,
                vk::SamplerAddressMode::REPEAT
                    | vk::SamplerAddressMode::MIRRORED_REPEAT
                    | vk::SamplerAddressMode::CLAMP_TO_EDGE
                    | vk::SamplerAddressMode::CLAMP_TO_BORDER
            ) {
                return None;
            }
            wrap[i] = value.as_raw();
        }
        Some(Self {
            min: min.as_raw(),
            mag: mag.as_raw(),
            wrap,
        })
    }
}

pub(crate) struct Sampler {
    context: Arc<Context>,
    pub key: SamplerKey,
    pub handle: vk::Sampler,
}
impl Sampler {
    pub fn new(context: &Arc<Context>, key: SamplerKey) -> Option<Self> {
        let handle = unsafe {
            context
                .device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .min_filter(vk::Filter::from_raw(key.min))
                        .mag_filter(vk::Filter::from_raw(key.mag))
                        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                        .address_mode_u(vk::SamplerAddressMode::from_raw(key.wrap[0]))
                        .address_mode_v(vk::SamplerAddressMode::from_raw(key.wrap[1]))
                        .address_mode_w(vk::SamplerAddressMode::from_raw(key.wrap[2]))
                        .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK)
                        .min_lod(0.0)
                        .max_lod(0.0),
                    None,
                )
                .ok()?
        };
        Some(Self {
            context: context.clone(),
            key,
            handle,
        })
    }
}
impl Drop for Sampler {
    fn drop(&mut self) {
        unsafe {
            self.context.device.destroy_sampler(self.handle, None);
        }
    }
}
