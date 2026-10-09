//! Shared bounded worker service. Evidence: provenance 0026 and 0031.
pub(crate) use crate::workers::AsyncPipelines;
pub use crate::workers::RequestError;
pub type PipelineRequest<P = super::pipelines::ComputePipeline> =
    crate::workers::PipelineRequest<P>;
pub type PipelineStatus<P = super::pipelines::ComputePipeline> = crate::workers::PipelineStatus<P>;
