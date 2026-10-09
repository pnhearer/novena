//! Explicit host interpretations of opaque texture fields. Provenance: 0031.
use crate::{
    api::TextureDescription,
    tiling::{BlockFormat, ImageKind, ImageShape, Layout, TileShape},
};
use ash::vk;
use std::collections::HashSet;

/// Storage selected by a host after interpreting recorded builder fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Storage {
    Linear,
    Tiled(TileShape),
}

/// One complete flags/target/format tuple. No guest enum is implicit.
#[derive(Clone, Copy)]
pub struct ImageRule {
    pub flags: u64,
    pub target: u64,
    pub format: u64,
    pub host_format: vk::Format,
    pub kind: ImageKind,
    pub storage: Storage,
}

#[derive(Clone, Default)]
pub struct ImageContract {
    pub rules: Vec<ImageRule>,
}

#[derive(Clone)]
pub struct ImageDescriptor {
    pub shape: ImageShape,
    pub format: vk::Format,
}
impl ImageDescriptor {
    pub fn packing(&self, tile: TileShape) -> Option<Layout> {
        Layout::new(self.shape, format_block(self.format)?, tile).ok()
    }
}

pub(crate) struct ResolvedImage {
    pub descriptor: ImageDescriptor,
    pub packing: Layout,
    pub storage: Storage,
}
impl ImageContract {
    pub fn validate(&self) -> Result<(), String> {
        let mut keys = HashSet::new();
        for r in &self.rules {
            if !keys.insert((r.flags, r.target, r.format)) || format_block(r.host_format).is_none()
            {
                return Err("duplicate image rule or unsupported format geometry".into());
            }
            if let Storage::Tiled(t) = r.storage {
                if t.height_log2 > 5 || t.depth_log2 > 5 {
                    return Err("image tile exponent exceeds five".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn resolve(&self, d: &TextureDescription) -> Option<ResolvedImage> {
        let r = self
            .rules
            .iter()
            .find(|r| (r.flags, r.target, r.format) == (d.flags, d.target, d.format))?;
        let width = u32::try_from(d.width).ok()?;
        let height = u32::try_from(d.height).ok()?;
        let count = u32::try_from(d.depth.max(1)).ok()?;
        let (depth, layers) = if r.kind == ImageKind::D3 {
            (count, 1)
        } else if r.kind == ImageKind::D2 {
            (1, 1)
        } else {
            (1, count)
        };
        if r.kind == ImageKind::D2 && count != 1 {
            return None;
        }
        let descriptor = ImageDescriptor {
            shape: ImageShape {
                width,
                height,
                depth,
                layers,
                levels: u32::try_from(d.levels.max(1)).ok()?,
                kind: r.kind,
            },
            format: r.host_format,
        };
        let tile = match r.storage {
            Storage::Linear => TileShape {
                height_log2: 0,
                depth_log2: 0,
            },
            Storage::Tiled(t) => t,
        };
        let packing = descriptor.packing(tile)?;
        // A nonzero stride is accepted only for the packed single-level linear case.
        if d.stride != 0
            && (r.storage != Storage::Linear
                || descriptor.shape.levels != 1
                || d.stride != packing.levels()[0].row_bytes as u64)
        {
            return None;
        }
        Some(ResolvedImage {
            descriptor,
            packing,
            storage: r.storage,
        })
    }
}

/// Public host format block geometry. Payloads stay opaque during transfers.
pub fn format_block(format: vk::Format) -> Option<BlockFormat> {
    use vk::Format as F;
    let bytes = match format {
        F::R8_UNORM | F::R8_SNORM | F::R8_UINT | F::R8_SINT | F::R8_SRGB => 1,
        F::R8G8_UNORM
        | F::R8G8_SNORM
        | F::R8G8_UINT
        | F::R8G8_SINT
        | F::R8G8_SRGB
        | F::R16_UNORM
        | F::R16_SNORM
        | F::R16_UINT
        | F::R16_SINT
        | F::R16_SFLOAT => 2,
        F::R8G8B8A8_UNORM
        | F::R8G8B8A8_SNORM
        | F::R8G8B8A8_UINT
        | F::R8G8B8A8_SINT
        | F::R8G8B8A8_SRGB
        | F::B8G8R8A8_UNORM
        | F::B8G8R8A8_SRGB
        | F::R16G16_UNORM
        | F::R16G16_SNORM
        | F::R16G16_UINT
        | F::R16G16_SINT
        | F::R16G16_SFLOAT
        | F::R32_UINT
        | F::R32_SINT
        | F::R32_SFLOAT
        | F::D32_SFLOAT
        | F::A2B10G10R10_UNORM_PACK32
        | F::A2R10G10B10_UNORM_PACK32 => 4,
        F::R16G16B16A16_UNORM
        | F::R16G16B16A16_SNORM
        | F::R16G16B16A16_UINT
        | F::R16G16B16A16_SINT
        | F::R16G16B16A16_SFLOAT
        | F::R32G32_UINT
        | F::R32G32_SINT
        | F::R32G32_SFLOAT => 8,
        F::R32G32B32A32_UINT | F::R32G32B32A32_SINT | F::R32G32B32A32_SFLOAT => 16,
        _ => {
            let raw = format.as_raw();
            if (F::BC1_RGB_UNORM_BLOCK.as_raw()..=F::BC7_SRGB_BLOCK.as_raw()).contains(&raw) {
                let bytes = if matches!(
                    format,
                    F::BC1_RGB_UNORM_BLOCK
                        | F::BC1_RGB_SRGB_BLOCK
                        | F::BC1_RGBA_UNORM_BLOCK
                        | F::BC1_RGBA_SRGB_BLOCK
                        | F::BC4_UNORM_BLOCK
                        | F::BC4_SNORM_BLOCK
                ) {
                    8
                } else {
                    16
                };
                return Some(BlockFormat {
                    width: 4,
                    height: 4,
                    bytes,
                });
            }
            let first = F::ASTC_4X4_UNORM_BLOCK.as_raw();
            let last = F::ASTC_12X12_SRGB_BLOCK.as_raw();
            if (first..=last).contains(&raw) {
                const EXTENTS: [(u8, u8); 14] = [
                    (4, 4),
                    (5, 4),
                    (5, 5),
                    (6, 5),
                    (6, 6),
                    (8, 5),
                    (8, 6),
                    (8, 8),
                    (10, 5),
                    (10, 6),
                    (10, 8),
                    (10, 10),
                    (12, 10),
                    (12, 12),
                ];
                let (width, height) = EXTENTS[((raw - first) / 2) as usize];
                return Some(BlockFormat {
                    width,
                    height,
                    bytes: 16,
                });
            }
            return None;
        }
    };
    Some(BlockFormat {
        width: 1,
        height: 1,
        bytes,
    })
}

/// A checked texel-space box in one image mip and array layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRegion {
    pub level: u32,
    pub layer: u32,
    pub offset: [u32; 3],
    pub extent: [u32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlitRegion {
    pub level: u32,
    pub layer: u32,
    pub offsets: [[i32; 3]; 2],
}

/// Host interpretation of a recorded opaque image-copy argument set.
#[derive(Clone, Copy)]
pub enum CopyOperation {
    Copy {
        source: u64,
        destination: u64,
        from: ImageRegion,
        to: ImageRegion,
    },
    Blit {
        source: u64,
        destination: u64,
        from: BlitRegion,
        to: BlitRegion,
        filter: vk::Filter,
    },
}

pub type CopyDecoder = std::sync::Arc<dyn Fn([u64; 8]) -> Option<CopyOperation> + Send + Sync>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericClass {
    Float,
    Unsigned,
    Signed,
    Depth,
}
pub(crate) fn numeric_class(format: vk::Format) -> NumericClass {
    use vk::Format as F;
    match format {
        F::R8_UINT
        | F::R8G8_UINT
        | F::R8G8B8A8_UINT
        | F::R16_UINT
        | F::R16G16_UINT
        | F::R16G16B16A16_UINT
        | F::R32_UINT
        | F::R32G32_UINT
        | F::R32G32B32A32_UINT => NumericClass::Unsigned,
        F::R8_SINT
        | F::R8G8_SINT
        | F::R8G8B8A8_SINT
        | F::R16_SINT
        | F::R16G16_SINT
        | F::R16G16B16A16_SINT
        | F::R32_SINT
        | F::R32G32_SINT
        | F::R32G32B32A32_SINT => NumericClass::Signed,
        F::D32_SFLOAT | F::D32_SFLOAT_S8_UINT => NumericClass::Depth,
        _ => NumericClass::Float,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_builder_fields_need_an_exact_rule() {
        let rule = ImageRule {
            flags: 17,
            target: 19,
            format: 23,
            host_format: vk::Format::BC1_RGBA_UNORM_BLOCK,
            kind: ImageKind::D2Array,
            storage: Storage::Tiled(TileShape {
                height_log2: 3,
                depth_log2: 0,
            }),
        };
        let mut contract = ImageContract { rules: vec![rule] };
        contract.validate().unwrap();
        let mut description = TextureDescription {
            flags: 17,
            target: 19,
            format: 23,
            width: 37,
            height: 29,
            depth: 3,
            levels: 6,
            ..Default::default()
        };
        let image = contract.resolve(&description).unwrap();
        assert_eq!(image.descriptor.shape.layers, 3);
        assert_eq!(image.packing.levels()[0].blocks, [10, 8, 1]);
        assert_eq!(image.packing.levels()[0].tile.height_log2, 0);
        description.flags = 18;
        assert!(contract.resolve(&description).is_none());
        description.flags = 17;
        description.stride = 80;
        assert!(contract.resolve(&description).is_none());
        contract.rules.push(rule);
        assert!(contract.validate().is_err());
    }
}
