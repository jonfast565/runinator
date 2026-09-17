use runinator_models::value::Value;
use serde::{Deserialize, Serialize};

mod artifact_content_response;
pub use artifact_content_response::ArtifactContentResponse;

mod ingress_response;
pub use ingress_response::IngressResponse;

mod pipeline_ingress_request;
pub use pipeline_ingress_request::PipelineIngressRequest;
