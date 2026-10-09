//! Bounded sampled images and explicit host interpretations. Provenance: 0030.
use super::{
    graphics::{instructions, mapped},
    pipelines::stage_bindings,
    uniforms::UniformStage,
    Context,
};
use crate::{api::SamplerDescription, tiling::ImageKind};
use ash::vk;
use std::{collections::HashSet, sync::Arc};

/// One recorded slot mapped to the translator's original descriptor pair.
#[derive(Clone, Copy, Debug)]
pub struct TextureMapping {
    /// Raw stage token supplied by the caller.
    pub stage: u64,
    /// Raw binding index supplied by the caller.
    pub index: u64,
    /// Translated shader stage receiving the descriptor pair.
    pub target: UniformStage,
    /// Descriptor set number in the translated module.
    pub set: u32,
    /// Sampled-image descriptor binding number in the translated module.
    pub image: u32,
    /// Separate sampler descriptor binding number in the translated module.
    pub sampler: u32,
}

/// Host choices for unresolved enums and bindings. No implicit guest mapping.
#[derive(Clone, Default)]
pub struct TextureContract {
    /// Explicit mappings used to connect recorded slots to host resources.
    pub bindings: Vec<TextureMapping>,
    /// Explicit mapping from caller filter tokens to Vulkan filters.
    pub filters: Vec<(u64, vk::Filter)>,
    /// Explicit mapping from caller wrap tokens to Vulkan address modes.
    pub wraps: Vec<(u64, vk::SamplerAddressMode)>,
    /// Raw token accepted as disabled comparison sampling.
    pub compare_disabled: u64,
    /// Opt into the recorded LOD float order and an explicit mip filter.
    pub lod: Option<vk::SamplerMipmapMode>,
    /// Explicit combined-handle interpretation: handle, texture id, sampler id.
    /// Separate handles returned by this library use registration ids.
    pub combined: Vec<(u64, u32, u32)>,
}
impl TextureContract {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.lod.is_some_and(|m| {
            !matches!(
                m,
                vk::SamplerMipmapMode::NEAREST | vk::SamplerMipmapMode::LINEAR
            )
        }) {
            return Err("unsupported mip filter".into());
        }
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
    /// Translated shader stage.
    pub stage: UniformStage,
    /// Descriptor set in the translated module.
    pub set: u32,
    /// Descriptor binding number in the translated module.
    pub binding: u32,
    /// Reflected sampled-image or sampler descriptor type.
    pub ty: vk::DescriptorType,
    /// Reflected image dimension, present for sampled-image bindings.
    pub image_kind: Option<ImageKind>,
    /// Whether the reflected image type is an array.
    pub arrayed: bool,
}
impl Binding {
    /// Combine source set and binding into the flattened descriptor binding.
    pub fn lowered(self) -> u32 {
        self.set * 256 + self.binding
    }
    /// Return the descriptor set used for this stage in the graphics pipeline.
    pub fn stage_set(self) -> u32 {
        self.stage.set() + 2
    }
}

/// Reflect separate float images and samplers with their image view shapes.
pub(crate) fn bindings(words: &[u32], stage: UniformStage) -> Result<Vec<Binding>, String> {
    let reflected = stage_bindings(words, stage.model())?;
    let instructions = instructions(words)?;
    for &(op, args) in &instructions {
        if op == 25
            && (args.len() != 8
                || args[3] != 0
                || args[5] != 0
                || args[6] != 1
                || args[7] != 0
                || !matches!((args[2], args[4]), (1, 0 | 1) | (2, 0) | (3, 0 | 1))
                || !instructions.contains(&(22, &[args[1], 32])))
        {
            return Err("unsupported sampled image type".into());
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
            let mut image_kind = None;
            let mut arrayed = false;
            if b.descriptor_type == vk::DescriptorType::SAMPLED_IMAGE {
                let variable = instructions
                    .iter()
                    .find_map(|(op, a)| {
                        if *op != 59 || a.len() < 3 {
                            return None;
                        }
                        let id = a[1];
                        let set = instructions
                            .iter()
                            .any(|(op, d)| *op == 71 && *d == [id, 34, b.set]);
                        let binding = instructions
                            .iter()
                            .any(|(op, d)| *op == 71 && *d == [id, 33, b.binding]);
                        (set && binding).then_some(a[0])
                    })
                    .ok_or("missing image variable")?;
                let pointer = instructions
                    .iter()
                    .find(|(op, a)| *op == 32 && a.first() == Some(&variable))
                    .ok_or("missing image pointer")?
                    .1;
                let mut ty = pointer[2];
                for _ in 0..instructions.len() {
                    let (op, a) = instructions
                        .iter()
                        .find(|(op, a)| matches!(*op, 25 | 28) && a.first() == Some(&ty))
                        .ok_or("missing image type")?;
                    if *op == 28 {
                        ty = a[1];
                        continue;
                    }
                    if *op != 25 {
                        return Err("invalid image descriptor type".into());
                    }
                    arrayed = a[4] != 0;
                    image_kind = Some(match (a[2], arrayed) {
                        (1, false) => ImageKind::D2,
                        (1, true) => ImageKind::D2Array,
                        (2, false) => ImageKind::D3,
                        (3, _) => ImageKind::Cube,
                        _ => return Err("unsupported image shape".into()),
                    });
                    break;
                }
            }
            Ok(Binding {
                stage,
                set: b.set,
                binding: b.binding,
                ty: b.descriptor_type,
                image_kind,
                arrayed,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SamplerKey {
    min: i32,
    mag: i32,
    wrap: [i32; 3],
    lod: [u32; 3],
    mipmap: i32,
}
impl SamplerKey {
    /// Create a checked sampler interpretation or Vulkan sampler; return None for unsupported state.
    pub fn new(d: &SamplerDescription, c: &TextureContract) -> Option<Self> {
        if d.compare_mode != c.compare_disabled
            || d.max_anisotropy != 1.0
            || (c.lod.is_none() && (d.lod_bias != 0.0 || d.lod_clamp != [0.0; 2]))
            || !d.lod_bias.is_finite()
            || !d.lod_clamp.iter().all(|v| v.is_finite())
            || d.lod_clamp[0] < 0.0
            || d.lod_clamp[1] < d.lod_clamp[0]
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
            lod: [
                d.lod_clamp[0].to_bits(),
                d.lod_clamp[1].to_bits(),
                d.lod_bias.to_bits(),
            ],
            mipmap: c.lod.unwrap_or(vk::SamplerMipmapMode::NEAREST).as_raw(),
        })
    }
}

pub(crate) struct Sampler {
    context: Arc<Context>,
    /// Complete interpreted sampler configuration.
    pub key: SamplerKey,
    /// Owned Vulkan sampler handle.
    pub handle: vk::Sampler,
}
impl Sampler {
    /// Create a checked sampler interpretation or Vulkan sampler; return None for unsupported state.
    pub fn new(context: &Arc<Context>, key: SamplerKey) -> Option<Self> {
        let limits = unsafe {
            context
                .instance
                .get_physical_device_properties(context.physical_device)
        }
        .limits;
        if f32::from_bits(key.lod[2]).abs() > limits.max_sampler_lod_bias {
            return None;
        }
        let handle = unsafe {
            context
                .device
                .create_sampler(
                    &vk::SamplerCreateInfo::default()
                        .min_filter(vk::Filter::from_raw(key.min))
                        .mag_filter(vk::Filter::from_raw(key.mag))
                        .mipmap_mode(vk::SamplerMipmapMode::from_raw(key.mipmap))
                        .address_mode_u(vk::SamplerAddressMode::from_raw(key.wrap[0]))
                        .address_mode_v(vk::SamplerAddressMode::from_raw(key.wrap[1]))
                        .address_mode_w(vk::SamplerAddressMode::from_raw(key.wrap[2]))
                        .border_color(vk::BorderColor::FLOAT_TRANSPARENT_BLACK)
                        .min_lod(f32::from_bits(key.lod[0]))
                        .max_lod(f32::from_bits(key.lod[1]))
                        .mip_lod_bias(f32::from_bits(key.lod[2])),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mip_bounds_require_explicit_host_order_and_finite_values() {
        let mut contract = TextureContract {
            filters: vec![(7, vk::Filter::LINEAR)],
            wraps: vec![(9, vk::SamplerAddressMode::REPEAT)],
            compare_disabled: 11,
            ..Default::default()
        };
        let mut description = SamplerDescription {
            min_filter: 7,
            mag_filter: 7,
            wrap: [9; 3],
            max_anisotropy: 1.0,
            compare_mode: 11,
            lod_clamp: [0.0, 5.0],
            ..Default::default()
        };
        assert!(SamplerKey::new(&description, &contract).is_none());
        contract.lod = Some(vk::SamplerMipmapMode::LINEAR);
        let first = SamplerKey::new(&description, &contract).unwrap();
        description.lod_bias = 0.5;
        assert_ne!(
            first.lod,
            SamplerKey::new(&description, &contract).unwrap().lod
        );
        description.lod_clamp = [5.0, 1.0];
        assert!(SamplerKey::new(&description, &contract).is_none());
        description.lod_clamp = [0.0, f32::INFINITY];
        assert!(SamplerKey::new(&description, &contract).is_none());
        description.lod_clamp = [0.0, 1.0];
        description.lod_bias = f32::NAN;
        assert!(SamplerKey::new(&description, &contract).is_none());
    }
}
