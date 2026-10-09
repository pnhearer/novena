//! Public translator cache adapter shared by startup integration fixtures.
use novena::{
    startup_cache::{TranslatedShader, TranslationContext},
    ShaderTranslator,
};
use shadowbox::{
    interface::{
        GeometryInput, GraphicsState, TessellationDomain, TessellationSpacing, TessellationState,
    },
    CacheContext, TargetGeneration, TranslationOptions,
};
use std::sync::atomic::{AtomicUsize, Ordering};

fn context(context: &TranslationContext) -> Result<CacheContext<'_>, String> {
    let generation = match context.generation {
        0 => TargetGeneration::Sm5,
        1 => TargetGeneration::Sm6,
        2 => TargetGeneration::Sm7_5Plus,
        _ => return Err("invalid target generation".into()),
    };
    let geometry_input = context
        .geometry_input
        .map(|input| match input {
            1 => Ok(GeometryInput::Points),
            2 => Ok(GeometryInput::Lines),
            3 => Ok(GeometryInput::LinesAdjacency),
            4 => Ok(GeometryInput::Triangles),
            5 => Ok(GeometryInput::TrianglesAdjacency),
            _ => Err("invalid geometry input".to_string()),
        })
        .transpose()?;
    let tessellation = context
        .tessellation
        .map(|[domain, spacing, clockwise]| {
            if clockwise > 1 {
                return Err("invalid tessellation winding".to_string());
            }
            Ok(TessellationState {
                domain: match domain {
                    0 => TessellationDomain::Isolines,
                    1 => TessellationDomain::Triangles,
                    2 => TessellationDomain::Quads,
                    _ => return Err("invalid tessellation domain".into()),
                },
                spacing: match spacing {
                    0 => TessellationSpacing::Equal,
                    1 => TessellationSpacing::FractionalEven,
                    2 => TessellationSpacing::FractionalOdd,
                    _ => return Err("invalid tessellation spacing".into()),
                },
                clockwise: clockwise != 0,
            })
        })
        .transpose()?;
    Ok(CacheContext {
        generation,
        options: TranslationOptions {
            global_delta_zero: context.global_delta_zero,
        },
        graphics_state: GraphicsState {
            geometry_input,
            tessellation,
            ..Default::default()
        },
    })
}

pub(crate) struct Adapter(pub(crate) AtomicUsize);
impl ShaderTranslator for Adapter {
    fn translate(&self, _: u32, program: &[u8]) -> Result<Vec<u32>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        shadowbox::translate_header_prefixed(program)
            .map(|o| o.spirv)
            .map_err(|e| e.to_string())
    }
    fn translation_cache_key(
        &self,
        _: u32,
        program: &[u8],
        state: &TranslationContext,
    ) -> Result<Option<String>, String> {
        let context = context(state)?;
        let input = shadowbox::header_prefixed_input(program, context.generation)
            .map_err(|e| e.to_string())?;
        let mut header = [0; 128];
        let length = header.len().min(input.program_record.len());
        header[..length].copy_from_slice(&input.program_record[..length]);
        let words: Vec<_> = header
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let stage = shadowbox::headers::parse(&words)
            .map_err(|e| format!("{e:?}"))?
            .stage;
        if !shadowbox::missing_state_keys(stage, &context.graphics_state).is_empty() {
            return Ok(None);
        }
        shadowbox::translation_cache_key(&input, context)
            .map(Some)
            .map_err(|e| e.to_string())
    }
    fn translate_for_context(
        &self,
        _: u32,
        program: &[u8],
        state: &TranslationContext,
    ) -> Result<TranslatedShader, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        let context = context(state)?;
        let input = shadowbox::header_prefixed_input(program, context.generation)
            .map_err(|e| e.to_string())?;
        let output = shadowbox::translate_with_options_and_graphics_state(
            &input,
            context.options,
            &context.graphics_state,
        )
        .map_err(|e| e.to_string())?;
        Ok(TranslatedShader {
            key: String::new(),
            words: output.spirv,
            requires_subgroup_size_32: output.requires_subgroup_size_32,
        })
    }
}
