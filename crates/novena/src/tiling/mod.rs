//! CPU conversion between packed-linear image bytes and a block-linear layout.
//! Provenance: 0031.

use std::{error::Error, fmt, thread};

const GOB_WIDTH: usize = 64;
const GOB_HEIGHT: usize = 8;
const GOB_SIZE: usize = GOB_WIDTH * GOB_HEIGHT;
const SECTOR_SIZE: usize = 16;
const MAX_LEVELS: u32 = 16;
const PARALLEL_MIN_BYTES: usize = 256 * 1024;
const STREAM_MIN_BYTES: usize = 1024 * 1024;
const SECTOR_OFFSETS: [[usize; 4]; 8] = [
    [0, 32, 256, 288],
    [16, 48, 272, 304],
    [64, 96, 320, 352],
    [80, 112, 336, 368],
    [128, 160, 384, 416],
    [144, 176, 400, 432],
    [192, 224, 448, 480],
    [208, 240, 464, 496],
];

/// Image dimensions and array organization supported by checked packing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageKind {
    /// One two-dimensional image with depth and layer count one.
    D2,
    /// Array of two-dimensional images with depth one.
    D2Array,
    /// One three-dimensional image with layer count one.
    D3,
    /// Square cube faces in layers; the layer count is divisible by six.
    Cube,
}

/// Base texel dimensions, layers, and complete or partial mip chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageShape {
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Depth in texels.
    pub depth: u32,
    /// Number of array layers; cube faces count as layers.
    pub layers: u32,
    /// Borrow mip layouts in increasing level order.
    pub levels: u32,
    /// Image dimension and layer organization.
    pub kind: ImageKind,
}

/// Opaque format-block geometry used for packing without decoding payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockFormat {
    /// Width of one format block in texels.
    pub width: u8,
    /// Height of one format block in texels.
    pub height: u8,
    /// Bytes per opaque format block, supported for sizes 1, 2, 4, 8, and 16.
    pub bytes: u8,
}

/// Block-linear tile exponents in units of GOBs, each limited to zero through five.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileShape {
    /// Base-two exponent of the tile height in eight-row GOBs.
    pub height_log2: u8,
    /// Base-two exponent of the tile depth in GOBs.
    pub depth_log2: u8,
}

/// Checked storage geometry for one mip level within an array layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelLayout {
    /// Width, height, and depth of this mip in texels.
    pub extent: [u32; 3],
    /// Mip dimensions rounded up to whole format blocks.
    pub blocks: [u32; 3],
    /// Active format-block bytes in one row.
    pub row_bytes: usize,
    /// Effective tile after mip-dependent clamping.
    pub tile: TileShape,
    /// Byte offset of this mip within one tiled array layer.
    pub tiled_offset: usize,
    /// Total tiled storage size in bytes, including padding.
    pub tiled_size: usize,
    /// Bytes between tiled block rows.
    pub tiled_row_stride: usize,
    /// Bytes between tiled depth slabs.
    pub tiled_depth_stride: usize,
    /// Byte offset of this mip within one packed-linear layer.
    pub linear_offset: usize,
    /// Total packed-linear storage size in bytes, including padding.
    pub linear_size: usize,
}

/// Checked mip offsets, layer strides, and sizes for linear and tiled storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layout {
    shape: ImageShape,
    format: BlockFormat,
    levels: Box<[LevelLayout]>,
    array_stride: usize,
    linear_layer_stride: usize,
    tiled_size: usize,
    linear_size: usize,
}

/// Invalid image geometry, arithmetic overflow, or insufficient conversion storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// A dimension, layer count, or mip count is zero.
    ZeroDimension,
    /// Dimensions or layer counts do not match the selected image kind.
    InvalidShape,
    /// Format block geometry or element size is unsupported.
    InvalidFormat,
    /// A tile exponent exceeds five.
    InvalidTile,
    /// Offset or size arithmetic cannot be represented.
    Overflow,
    /// The source buffer is smaller than the required representation.
    SourceSize {
        /// Minimum required byte length.
        expected: usize,
        /// Actual supplied byte length.
        actual: usize,
    },
    /// The destination buffer is smaller than the required representation.
    DestinationSize {
        /// Minimum required byte length.
        expected: usize,
        /// Actual supplied byte length.
        actual: usize,
    },
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimension => {
                f.write_str("image dimensions, layers, and levels must be nonzero")
            }
            Self::InvalidShape => f.write_str("image dimensions do not match the image kind"),
            Self::InvalidFormat => f.write_str("unsupported block format"),
            Self::InvalidTile => f.write_str("tile exponents are out of range"),
            Self::Overflow => f.write_str("image layout size overflow"),
            Self::SourceSize { expected, actual } => {
                write!(f, "source is {actual} bytes, expected at least {expected}")
            }
            Self::DestinationSize { expected, actual } => {
                write!(
                    f,
                    "destination is {actual} bytes, expected at least {expected}"
                )
            }
        }
    }
}

impl Error for LayoutError {}

impl Layout {
    /// Compute checked packing; reject invalid shape, block geometry, tile exponents, or overflow.
    pub fn new(
        shape: ImageShape,
        format: BlockFormat,
        tile: TileShape,
    ) -> Result<Self, LayoutError> {
        validate(shape, format, tile)?;
        let mut levels = Vec::with_capacity(shape.levels as usize);
        let mut tiled_end = 0usize;
        let mut linear_end = 0usize;

        for level in 0..shape.levels {
            let extent = [
                mip_extent(shape.width, level),
                mip_extent(shape.height, level),
                if shape.kind == ImageKind::D3 {
                    mip_extent(shape.depth, level)
                } else {
                    1
                },
            ];
            let blocks = [
                div_ceil_u32(extent[0], u32::from(format.width)),
                div_ceil_u32(extent[1], u32::from(format.height)),
                extent[2],
            ];
            let row_bytes = usize::try_from(blocks[0])
                .ok()
                .and_then(|v| v.checked_mul(usize::from(format.bytes)))
                .ok_or(LayoutError::Overflow)?;
            let level_tile = TileShape {
                height_log2: clamp_log2(tile.height_log2, blocks[1], GOB_HEIGHT as u32),
                depth_log2: clamp_log2(tile.depth_log2, blocks[2], 1),
            };
            let tile_height = GOB_HEIGHT << level_tile.height_log2;
            let tile_depth = 1usize << level_tile.depth_log2;
            let tile_bytes = GOB_SIZE
                .checked_shl(u32::from(level_tile.height_log2 + level_tile.depth_log2))
                .ok_or(LayoutError::Overflow)?;
            let tile_columns = div_ceil(row_bytes, GOB_WIDTH);
            let tile_rows = div_ceil(blocks[1] as usize, tile_height);
            let tile_depths = div_ceil(blocks[2] as usize, tile_depth);
            let tiled_row_stride = tile_columns
                .checked_mul(tile_bytes)
                .ok_or(LayoutError::Overflow)?;
            let tiled_depth_stride = tile_rows
                .checked_mul(tiled_row_stride)
                .ok_or(LayoutError::Overflow)?;
            let tiled_size = tile_depths
                .checked_mul(tiled_depth_stride)
                .ok_or(LayoutError::Overflow)?;
            let tiled_offset = align_up(tiled_end, tile_bytes)?;
            tiled_end = tiled_offset
                .checked_add(tiled_size)
                .ok_or(LayoutError::Overflow)?;

            let active_linear_size = row_bytes
                .checked_mul(blocks[1] as usize)
                .and_then(|v| v.checked_mul(blocks[2] as usize))
                .ok_or(LayoutError::Overflow)?;
            let linear_offset = align_up(linear_end, SECTOR_SIZE)?;
            let linear_size = align_up(active_linear_size, SECTOR_SIZE)?;
            linear_end = linear_offset
                .checked_add(linear_size)
                .ok_or(LayoutError::Overflow)?;
            levels.push(LevelLayout {
                extent,
                blocks,
                row_bytes,
                tile: level_tile,
                tiled_offset,
                tiled_size,
                tiled_row_stride,
                tiled_depth_stride,
                linear_offset,
                linear_size,
            });
        }

        let layers = storage_layers(shape);
        let base_tile = levels[0].tile;
        let base_tile_bytes = GOB_SIZE
            .checked_shl(u32::from(base_tile.height_log2 + base_tile.depth_log2))
            .ok_or(LayoutError::Overflow)?;
        let array_stride = align_up(tiled_end, base_tile_bytes)?;
        let linear_layer_stride = align_up(linear_end, SECTOR_SIZE)?;
        let tiled_size = array_stride
            .checked_mul(layers)
            .ok_or(LayoutError::Overflow)?;
        let linear_size = linear_layer_stride
            .checked_mul(layers)
            .ok_or(LayoutError::Overflow)?;

        Ok(Self {
            shape,
            format,
            levels: levels.into_boxed_slice(),
            array_stride,
            linear_layer_stride,
            tiled_size,
            linear_size,
        })
    }

    /// Return the base image shape used to construct this layout.
    pub fn shape(&self) -> ImageShape {
        self.shape
    }

    /// Return the opaque format-block geometry.
    pub fn format(&self) -> BlockFormat {
        self.format
    }

    /// Borrow mip layouts in increasing level order.
    pub fn levels(&self) -> &[LevelLayout] {
        &self.levels
    }

    /// Byte distance between consecutive tiled array layers.
    pub fn array_stride(&self) -> usize {
        self.array_stride
    }

    /// Byte distance between consecutive packed-linear array layers.
    pub fn linear_layer_stride(&self) -> usize {
        self.linear_layer_stride
    }

    /// Total tiled storage size in bytes, including padding.
    pub fn tiled_size(&self) -> usize {
        self.tiled_size
    }

    /// Total packed-linear storage size in bytes, including padding.
    pub fn linear_size(&self) -> usize {
        self.linear_size
    }

    /// Resolve layer, mip, block coordinates, and byte-in-block to a checked tiled offset.
    pub fn tiled_byte_offset(
        &self,
        level: u32,
        layer: u32,
        x_byte: u32,
        y: u32,
        z: u32,
    ) -> Option<usize> {
        let level = self.levels.get(level as usize)?;
        if layer as usize >= storage_layers(self.shape)
            || x_byte as usize >= level.row_bytes
            || y >= level.blocks[1]
            || z >= level.blocks[2]
        {
            return None;
        }
        let h = level.tile.height_log2 as usize;
        let d = level.tile.depth_log2 as usize;
        let x = x_byte as usize;
        let y = y as usize;
        let z = z as usize;
        let tile_columns = div_ceil(level.row_bytes, GOB_WIDTH);
        let tile_x = x / GOB_WIDTH;
        let gob_y = y / GOB_HEIGHT;
        let tile_y = gob_y >> h;
        let tile_z = z >> d;
        let gob_y_in_tile = gob_y & ((1usize << h) - 1);
        let gob_z_in_tile = z & ((1usize << d) - 1);
        let tile_bytes = GOB_SIZE << (h + d);
        let tile_index = tile_z
            .checked_mul(level.tiled_depth_stride / tile_bytes)?
            .checked_add(tile_y.checked_mul(tile_columns)?)?
            .checked_add(tile_x)?;
        let gob_index = (gob_z_in_tile << h) | gob_y_in_tile;
        (layer as usize)
            .checked_mul(self.array_stride)?
            .checked_add(level.tiled_offset)?
            .checked_add(tile_index.checked_mul(tile_bytes)?)?
            .checked_add(gob_index.checked_mul(GOB_SIZE)?)?
            .checked_add(sector_offset(x % GOB_WIDTH, y % GOB_HEIGHT))
    }

    /// Resolve layer, mip, block coordinates, and byte-in-block to a checked linear offset.
    pub fn linear_byte_offset(
        &self,
        level: u32,
        layer: u32,
        x_byte: u32,
        y: u32,
        z: u32,
    ) -> Option<usize> {
        let level = self.levels.get(level as usize)?;
        if layer as usize >= storage_layers(self.shape)
            || x_byte as usize >= level.row_bytes
            || y >= level.blocks[1]
            || z >= level.blocks[2]
        {
            return None;
        }
        (layer as usize)
            .checked_mul(self.linear_layer_stride)?
            .checked_add(level.linear_offset)?
            .checked_add(
                (z as usize)
                    .checked_mul(level.blocks[1] as usize)?
                    .checked_mul(level.row_bytes)?,
            )?
            .checked_add((y as usize).checked_mul(level.row_bytes)?)?
            .checked_add(x_byte as usize)
    }

    /// Copy active bytes from tiled to linear storage; preserve destination padding.
    pub fn decode(&self, tiled: &[u8], linear: &mut [u8]) -> Result<(), LayoutError> {
        self.check_buffers(tiled, linear, true)?;
        self.convert_serial(tiled, linear, true)
    }

    /// Copy active bytes from linear to tiled storage; preserve destination padding.
    pub fn encode(&self, linear: &[u8], tiled: &mut [u8]) -> Result<(), LayoutError> {
        self.check_buffers(linear, tiled, false)?;
        self.convert_serial(linear, tiled, false)
    }

    /// Decode disjoint ranges with at most the requested worker count; small images run serially.
    pub fn decode_parallel(
        &self,
        tiled: &[u8],
        linear: &mut [u8],
        threads: usize,
    ) -> Result<(), LayoutError> {
        self.check_buffers(tiled, linear, true)?;
        self.convert_parallel(tiled, linear, true, threads)
    }

    /// Encode disjoint ranges with at most the requested worker count; small images run serially.
    pub fn encode_parallel(
        &self,
        linear: &[u8],
        tiled: &mut [u8],
        threads: usize,
    ) -> Result<(), LayoutError> {
        self.check_buffers(linear, tiled, false)?;
        self.convert_parallel(linear, tiled, false, threads)
    }

    fn check_buffers(
        &self,
        source: &[u8],
        destination: &[u8],
        decode: bool,
    ) -> Result<(), LayoutError> {
        let (source_size, destination_size) = if decode {
            (self.tiled_size, self.linear_size)
        } else {
            (self.linear_size, self.tiled_size)
        };
        if source.len() < source_size {
            return Err(LayoutError::SourceSize {
                expected: source_size,
                actual: source.len(),
            });
        }
        if destination.len() < destination_size {
            return Err(LayoutError::DestinationSize {
                expected: destination_size,
                actual: destination.len(),
            });
        }
        Ok(())
    }

    fn convert_serial(
        &self,
        source: &[u8],
        destination: &mut [u8],
        decode: bool,
    ) -> Result<(), LayoutError> {
        for layer in 0..storage_layers(self.shape) as u32 {
            for level in 0..self.levels.len() as u32 {
                self.convert_level(source, destination, decode, layer, level, 0)?;
            }
        }
        Ok(())
    }

    fn convert_parallel(
        &self,
        source: &[u8],
        destination: &mut [u8],
        decode: bool,
        threads: usize,
    ) -> Result<(), LayoutError> {
        let work = storage_layers(self.shape) * self.levels.len();
        let workers = threads.max(1).min(work.max(1));
        if self.linear_size < PARALLEL_MIN_BYTES {
            return self.convert_serial(source, destination, decode);
        }
        if self.shape.kind == ImageKind::D2 && threads > 1 {
            for level in 0..self.levels.len() as u32 {
                if self.levels[level as usize].linear_size < PARALLEL_MIN_BYTES {
                    self.convert_level(source, destination, decode, 0, level, 0)?;
                } else {
                    self.convert_level_parallel(source, destination, decode, threads, level)?;
                }
            }
            return Ok(());
        }
        if self.shape.kind == ImageKind::D3 && self.levels.len() == 1 && threads > 1 {
            return self.convert_level_parallel(source, destination, decode, threads, 0);
        }
        if workers == 1 {
            return self.convert_serial(source, destination, decode);
        }
        let per_worker = div_ceil(work, workers);
        let destination_size = destination_len(self, decode);
        let mut rest = &mut destination[..destination_size];
        thread::scope(|scope| {
            let mut handles = Vec::new();
            for first in (0..work).step_by(per_worker) {
                let end = (first + per_worker).min(work);
                let start_offset = self.work_range(first, decode).0;
                let end_offset = self.work_range(end - 1, decode).1;
                let skip = start_offset - (destination_len(self, decode) - rest.len());
                let (_, after_skip) = rest.split_at_mut(skip);
                let (chunk, next) = after_skip.split_at_mut(end_offset - start_offset);
                rest = next;
                handles.push(scope.spawn(move || {
                    for index in first..end {
                        let layer = (index / self.levels.len()) as u32;
                        let level = (index % self.levels.len()) as u32;
                        self.convert_level(source, chunk, decode, layer, level, start_offset)?;
                    }
                    Ok(())
                }));
            }
            for handle in handles {
                handle.join().map_err(|_| LayoutError::InvalidShape)??;
            }
            Ok(())
        })
    }

    fn convert_level_parallel(
        &self,
        source: &[u8],
        destination: &mut [u8],
        decode: bool,
        threads: usize,
        level_index: u32,
    ) -> Result<(), LayoutError> {
        let level = self.levels[level_index as usize];
        let tile_height = GOB_HEIGHT << level.tile.height_log2;
        let tile_rows = div_ceil(level.blocks[1] as usize, tile_height);
        let tile_depth = 1usize << level.tile.depth_log2;
        let tile_depths = div_ceil(level.blocks[2] as usize, tile_depth);
        let split_depth = self.shape.kind == ImageKind::D3;
        let units = if split_depth { tile_depths } else { tile_rows };
        let workers = threads.min(units).max(1);
        let units_per_worker = div_ceil(units, workers);
        let tile_row_bytes = level.tiled_row_stride;
        thread::scope(|scope| {
            let mut handles = Vec::new();
            let mut consumed = 0usize;
            let mut rest = destination;
            for first in (0..units).step_by(units_per_worker) {
                let end = (first + units_per_worker).min(units);
                let start = if decode {
                    if split_depth {
                        level.linear_offset
                            + first * tile_depth * level.blocks[1] as usize * level.row_bytes
                    } else {
                        level.linear_offset + first * tile_height * level.row_bytes
                    }
                } else {
                    level.tiled_offset
                        + if split_depth {
                            first * level.tiled_depth_stride
                        } else {
                            first * tile_row_bytes
                        }
                };
                let finish = if decode {
                    if split_depth {
                        level.linear_offset
                            + (end * tile_depth).min(level.blocks[2] as usize)
                                * level.blocks[1] as usize
                                * level.row_bytes
                    } else {
                        level.linear_offset
                            + (end * tile_height).min(level.blocks[1] as usize) * level.row_bytes
                    }
                } else {
                    level.tiled_offset
                        + if split_depth {
                            end * level.tiled_depth_stride
                        } else {
                            end * tile_row_bytes
                        }
                };
                let (_, after_gap) = rest.split_at_mut(start - consumed);
                let (chunk, next) = after_gap.split_at_mut(finish - start);
                rest = next;
                consumed = finish;
                handles.push(scope.spawn(move || {
                    let (first_z, end_z, first_y, end_y) = if split_depth {
                        (first, end, 0, tile_rows)
                    } else {
                        (0, tile_depths, first, end)
                    };
                    self.convert_level_tiles(
                        source,
                        chunk,
                        decode,
                        0,
                        level_index,
                        start,
                        first_z,
                        end_z,
                        first_y,
                        end_y,
                    )
                }));
            }
            for handle in handles {
                handle.join().map_err(|_| LayoutError::InvalidShape)??;
            }
            Ok(())
        })
    }

    fn work_range(&self, index: usize, decode: bool) -> (usize, usize) {
        let layer = index / self.levels.len();
        let level = &self.levels[index % self.levels.len()];
        if decode {
            let start = layer * self.linear_layer_stride + level.linear_offset;
            (start, start + level.linear_size)
        } else {
            let start = layer * self.array_stride + level.tiled_offset;
            (start, start + level.tiled_size)
        }
    }

    fn convert_level(
        &self,
        source: &[u8],
        destination: &mut [u8],
        decode: bool,
        layer: u32,
        level_index: u32,
        destination_base: usize,
    ) -> Result<(), LayoutError> {
        let level = self.levels[level_index as usize];
        let tile_rows = div_ceil(
            level.blocks[1] as usize,
            GOB_HEIGHT << level.tile.height_log2,
        );
        self.convert_level_tiles(
            source,
            destination,
            decode,
            layer,
            level_index,
            destination_base,
            0,
            div_ceil(level.blocks[2] as usize, 1usize << level.tile.depth_log2),
            0,
            tile_rows,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn convert_level_tiles(
        &self,
        source: &[u8],
        destination: &mut [u8],
        decode: bool,
        layer: u32,
        level_index: u32,
        destination_base: usize,
        first_tile_depth: usize,
        end_tile_depth: usize,
        first_tile_row: usize,
        end_tile_row: usize,
    ) -> Result<(), LayoutError> {
        if decode {
            self.convert_level_direction::<true>(
                source,
                destination,
                layer,
                level_index,
                destination_base,
                first_tile_depth,
                end_tile_depth,
                first_tile_row,
                end_tile_row,
            )
        } else {
            self.convert_level_direction::<false>(
                source,
                destination,
                layer,
                level_index,
                destination_base,
                first_tile_depth,
                end_tile_depth,
                first_tile_row,
                end_tile_row,
            )
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn convert_level_direction<const DECODE: bool>(
        &self,
        source: &[u8],
        destination: &mut [u8],
        layer: u32,
        level_index: u32,
        destination_base: usize,
        first_tile_depth: usize,
        end_tile_depth: usize,
        first_tile_row: usize,
        end_tile_row: usize,
    ) -> Result<(), LayoutError> {
        let level = self.levels[level_index as usize];
        let h = level.tile.height_log2 as usize;
        let d = level.tile.depth_log2 as usize;
        let tile_bytes = GOB_SIZE << (h + d);
        let tile_columns = div_ceil(level.row_bytes, GOB_WIDTH);
        let tile_rows = div_ceil(level.blocks[1] as usize, GOB_HEIGHT << h);
        let tile_depths = div_ceil(level.blocks[2] as usize, 1usize << d);
        let tiled_level_base = layer as usize * self.array_stride + level.tiled_offset;
        let linear_level_base = layer as usize * self.linear_layer_stride + level.linear_offset;
        #[cfg(target_arch = "x86_64")]
        let stream = level.linear_size >= STREAM_MIN_BYTES
            && level.row_bytes.is_multiple_of(64)
            && (destination.as_ptr() as usize).is_multiple_of(16);
        #[cfg(not(target_arch = "x86_64"))]
        let stream = false;
        #[cfg(target_arch = "x86_64")]
        let prefix = if DECODE && stream {
            (64usize.wrapping_sub(
                (destination.as_ptr() as usize)
                    .wrapping_add(linear_level_base)
                    .wrapping_sub(destination_base),
            ) & 63)
                / 16
        } else {
            0
        };
        debug_assert!(end_tile_depth <= tile_depths);
        for tile_z in first_tile_depth..end_tile_depth {
            for tile_y in first_tile_row..end_tile_row {
                // Encode keeps eight linear source rows active; decode reads
                // consecutive tiled GOBs to keep its source stream contiguous.
                let bounds = if DECODE {
                    [tile_columns, 1usize << d, 1usize << h]
                } else {
                    [1usize << d, 1usize << h, tile_columns]
                };
                for first in 0..bounds[0] {
                    for second in 0..bounds[1] {
                        for third in 0..bounds[2] {
                            let (tile_x, gob_z, gob_in_y) = if DECODE {
                                (first, second, third)
                            } else {
                                (third, first, second)
                            };
                            let z = tile_z * (1usize << d) + gob_z;
                            let y = tile_y * (GOB_HEIGHT << h) + gob_in_y * GOB_HEIGHT;
                            if z >= level.blocks[2] as usize || y >= level.blocks[1] as usize {
                                continue;
                            }
                            let tile_index = (tile_z * tile_rows + tile_y) * tile_columns + tile_x;
                            let tile_base = tiled_level_base + tile_index * tile_bytes;
                            let x = tile_x * GOB_WIDTH;
                            let width = (level.row_bytes - x).min(GOB_WIDTH);
                            let height = (level.blocks[1] as usize - y).min(GOB_HEIGHT);
                            let tiled_base = tile_base + ((gob_z << h) | gob_in_y) * GOB_SIZE;
                            let linear_base = linear_level_base
                                + (z * level.blocks[1] as usize + y) * level.row_bytes
                                + x;
                            #[cfg(target_arch = "x86_64")]
                            if prefix != 0 {
                                // SAFETY: streaming selects complete 64-byte rows and
                                // 16-byte alignment. Only existing neighbor columns
                                // are read; prefix and tail writes stay in active rows.
                                unsafe {
                                    let src = source.as_ptr().add(tiled_base);
                                    let dst = destination
                                        .as_mut_ptr()
                                        .add(linear_base - destination_base);
                                    match prefix {
                                        1 => decode_shifted_gob::<1>(
                                            src,
                                            dst,
                                            level.row_bytes,
                                            tile_bytes,
                                            tile_x,
                                            tile_columns,
                                            height,
                                        ),
                                        2 => decode_shifted_gob::<2>(
                                            src,
                                            dst,
                                            level.row_bytes,
                                            tile_bytes,
                                            tile_x,
                                            tile_columns,
                                            height,
                                        ),
                                        3 => decode_shifted_gob::<3>(
                                            src,
                                            dst,
                                            level.row_bytes,
                                            tile_bytes,
                                            tile_x,
                                            tile_columns,
                                            height,
                                        ),
                                        _ => unreachable!(),
                                    }
                                }
                                continue;
                            }
                            if width == GOB_WIDTH && height == GOB_HEIGHT {
                                Self::convert_full_gob::<DECODE>(
                                    source,
                                    destination,
                                    tiled_base,
                                    linear_base,
                                    level.row_bytes,
                                    destination_base,
                                    stream,
                                )?;
                                continue;
                            }
                            for row in y..y + height {
                                for byte in (x..x + width).step_by(SECTOR_SIZE) {
                                    let count = (x + width - byte).min(SECTOR_SIZE);
                                    self.copy_sector(
                                        source,
                                        destination,
                                        DECODE,
                                        layer,
                                        level_index,
                                        byte,
                                        row,
                                        z as u32,
                                        count,
                                        destination_base,
                                    )?;
                                }
                            }
                        }
                    }
                }
            }
        }
        #[cfg(target_arch = "x86_64")]
        if stream {
            // SAFETY: the streaming stores above must become globally visible
            // before this worker returns and its destination may be consumed.
            unsafe { std::arch::x86_64::_mm_sfence() };
        }
        Ok(())
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn convert_full_gob<const DECODE: bool>(
        source: &[u8],
        destination: &mut [u8],
        tiled_base: usize,
        linear_base: usize,
        row_bytes: usize,
        destination_base: usize,
        stream: bool,
    ) -> Result<(), LayoutError> {
        #[cfg(not(target_arch = "x86_64"))]
        let _ = stream;
        let (source_base, destination_base) = if DECODE {
            (
                tiled_base,
                linear_base
                    .checked_sub(destination_base)
                    .ok_or(LayoutError::Overflow)?,
            )
        } else {
            (
                linear_base,
                tiled_base
                    .checked_sub(destination_base)
                    .ok_or(LayoutError::Overflow)?,
            )
        };
        #[cfg(target_arch = "x86_64")]
        if stream && !DECODE {
            // Pull the later half of the eight-row strip in before stores begin.
            // The first half is consumed immediately and benefits from demand loads.
            unsafe {
                use std::arch::x86_64::{_mm_prefetch, _MM_HINT_T0};
                _mm_prefetch(
                    source.as_ptr().add(source_base + 4 * row_bytes).cast(),
                    _MM_HINT_T0,
                );
            }
            for sector_group in [0usize, 2] {
                for row_pair in 0..4 {
                    for sector_in_group in 0..2 {
                        for row_in_pair in 0..2 {
                            let row = row_pair * 2 + row_in_pair;
                            let sector_index = sector_group + sector_in_group;
                            let sector = sector_index * SECTOR_SIZE;
                            let swizzled = SECTOR_OFFSETS[row][sector_index];
                            unsafe {
                                stream_copy_16(
                                    source.as_ptr().add(source_base + row * row_bytes + sector),
                                    destination.as_mut_ptr().add(destination_base + swizzled),
                                )
                            };
                        }
                    }
                }
            }
            return Ok(());
        }
        for (row, offsets) in SECTOR_OFFSETS.iter().enumerate() {
            for (sector_index, &swizzled) in offsets.iter().enumerate() {
                let sector = sector_index * SECTOR_SIZE;
                let (src, dst) = if DECODE {
                    (
                        source_base + swizzled,
                        destination_base + row * row_bytes + sector,
                    )
                } else {
                    (
                        source_base + row * row_bytes + sector,
                        destination_base + swizzled,
                    )
                };
                // The constructor and buffer checks prove both fixed-size regions
                // are in bounds. Full GOB sectors never overlap within one image.
                #[cfg(target_arch = "x86_64")]
                if stream {
                    // SAFETY: runtime checks prove 16-byte destination alignment;
                    // every offset is 16-byte aligned and the regions are in bounds.
                    unsafe {
                        stream_copy_16(source.as_ptr().add(src), destination.as_mut_ptr().add(dst))
                    };
                    continue;
                }
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        source.as_ptr().add(src),
                        destination.as_mut_ptr().add(dst),
                        SECTOR_SIZE,
                    )
                };
            }
        }
        Ok(())
    }

    #[inline(always)]
    #[allow(clippy::too_many_arguments)]
    fn copy_sector(
        &self,
        source: &[u8],
        destination: &mut [u8],
        decode: bool,
        layer: u32,
        level: u32,
        x: usize,
        y: usize,
        z: u32,
        count: usize,
        destination_base: usize,
    ) -> Result<(), LayoutError> {
        let tiled = self
            .tiled_byte_offset(level, layer, x as u32, y as u32, z)
            .ok_or(LayoutError::Overflow)?;
        let linear = self
            .linear_byte_offset(level, layer, x as u32, y as u32, z)
            .ok_or(LayoutError::Overflow)?;
        let (src, dst) = if decode {
            (
                tiled,
                linear
                    .checked_sub(destination_base)
                    .ok_or(LayoutError::Overflow)?,
            )
        } else {
            (
                linear,
                tiled
                    .checked_sub(destination_base)
                    .ok_or(LayoutError::Overflow)?,
            )
        };
        destination[dst..dst + count].copy_from_slice(&source[src..src + count]);
        Ok(())
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
#[allow(clippy::too_many_arguments)]
unsafe fn decode_shifted_gob<const PREFIX: usize>(
    source: *const u8,
    destination: *mut u8,
    row_bytes: usize,
    tile_bytes: usize,
    column: usize,
    columns: usize,
    height: usize,
) {
    for (row, offsets) in SECTOR_OFFSETS.iter().enumerate().take(height) {
        // SAFETY: caller supplies complete byte rows and valid GOB columns.
        unsafe {
            let dst = destination.add(row * row_bytes);
            if column == 0 {
                for (sector, &offset) in offsets.iter().enumerate().take(PREFIX) {
                    std::ptr::copy_nonoverlapping(source.add(offset), dst.add(sector * 16), 16);
                }
            }
            if column + 1 < columns {
                for sector in 0..4 {
                    let input_sector = PREFIX + sector;
                    let (gob, index) = if input_sector < 4 {
                        (0, input_sector)
                    } else {
                        (tile_bytes, input_sector - 4)
                    };
                    stream_copy_16(
                        source.add(gob + offsets[index]),
                        dst.add((PREFIX + sector) * 16),
                    );
                }
            } else {
                for (sector, &offset) in offsets.iter().enumerate().skip(PREFIX) {
                    std::ptr::copy_nonoverlapping(source.add(offset), dst.add(sector * 16), 16);
                }
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn stream_copy_16(source: *const u8, destination: *mut u8) {
    use std::arch::x86_64::{_mm_loadu_si128, _mm_stream_si128};
    let value = unsafe { _mm_loadu_si128(source.cast()) };
    unsafe { _mm_stream_si128(destination.cast(), value) };
}

fn validate(shape: ImageShape, format: BlockFormat, tile: TileShape) -> Result<(), LayoutError> {
    if shape.width == 0
        || shape.height == 0
        || shape.depth == 0
        || shape.layers == 0
        || shape.levels == 0
    {
        return Err(LayoutError::ZeroDimension);
    }
    let valid_shape = match shape.kind {
        ImageKind::D2 => shape.depth == 1 && shape.layers == 1,
        ImageKind::D2Array => shape.depth == 1,
        ImageKind::D3 => shape.layers == 1,
        ImageKind::Cube => {
            shape.depth == 1 && shape.width == shape.height && shape.layers.is_multiple_of(6)
        }
    };
    if !valid_shape {
        return Err(LayoutError::InvalidShape);
    }
    if format.width == 0 || format.height == 0 || !matches!(format.bytes, 1 | 2 | 4 | 8 | 16) {
        return Err(LayoutError::InvalidFormat);
    }
    if tile.height_log2 > 5 || tile.depth_log2 > 5 || shape.levels > MAX_LEVELS {
        return Err(LayoutError::InvalidTile);
    }
    Ok(())
}

fn storage_layers(shape: ImageShape) -> usize {
    if shape.kind == ImageKind::D3 {
        1
    } else {
        shape.layers as usize
    }
}

fn mip_extent(value: u32, level: u32) -> u32 {
    value.checked_shr(level).unwrap_or(0).max(1)
}

fn clamp_log2(mut log2: u8, extent: u32, unit: u32) -> u8 {
    while log2 > 0 && (unit << (log2 - 1)) >= extent {
        log2 -= 1;
    }
    log2
}

fn div_ceil(value: usize, divisor: usize) -> usize {
    value / divisor + usize::from(!value.is_multiple_of(divisor))
}

fn div_ceil_u32(value: u32, divisor: u32) -> u32 {
    value / divisor + u32::from(!value.is_multiple_of(divisor))
}

fn align_up(value: usize, alignment: usize) -> Result<usize, LayoutError> {
    let mask = alignment.checked_sub(1).ok_or(LayoutError::Overflow)?;
    value
        .checked_add(mask)
        .map(|v| v & !mask)
        .ok_or(LayoutError::Overflow)
}

fn destination_len(layout: &Layout, decode: bool) -> usize {
    if decode {
        layout.linear_size
    } else {
        layout.tiled_size
    }
}

#[inline(always)]
fn sector_offset(x: usize, y: usize) -> usize {
    x / 32 * 256 + y / 2 * 64 + (x % 32) / 16 * 32 + y % 2 * 16 + x % 16
}

#[cfg(test)]
mod tests;
