use super::*;

// input types ---------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    /// An awaitable durable-operation result. `None` represents the surface `task` generic
    /// elision; semantic analysis fills it from context when the type is used on a binding.
    Task(Option<Box<TypeExpr>>),
    Enum(Vec<runinator_models::value::Value>),
    Range {
        base: Box<TypeExpr>,
        min: Option<runinator_models::value::Value>,
        max: Option<runinator_models::value::Value>,
    },
    Array(Box<TypeExpr>),
    Map(Box<TypeExpr>),
    Struct {
        fields: Vec<TypeField>,
        additional: Option<Box<TypeExpr>>,
    },
    Union(Vec<TypeExpr>),
    /// a first-class function type `function<(params) -> ret>`, the surface form of the type a
    /// lambda infers. lowers to `RuninatorType::Function`.
    Function {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
}

// secrets (.rexraps) -----------------------------------------------------------

// pipelines (.rexrapp) ---------------------------------------------------------

mod secret_decl;
pub use secret_decl::SecretDecl;

mod profile_decl;
pub use profile_decl::ProfileDecl;

mod settings_document;
pub use settings_document::SettingsDocument;

mod pipeline_link_decl;
pub use pipeline_link_decl::PipelineLinkDecl;

mod pipeline_join_decl;
pub use pipeline_join_decl::PipelineJoinDecl;

mod pipeline_trigger_decl;
pub use pipeline_trigger_decl::PipelineTriggerDecl;

mod pipeline_member_decl;
pub use pipeline_member_decl::PipelineMemberDecl;

mod pipeline_decl;
pub use pipeline_decl::PipelineDecl;

mod orchestration_decl;
pub use orchestration_decl::OrchestrationDecl;

mod orchestration_intent_decl;
pub use orchestration_intent_decl::OrchestrationIntentDecl;

mod orchestration_budget_decl;
pub use orchestration_budget_decl::OrchestrationBudgetDecl;

mod orchestration_phase_decl;
pub use orchestration_phase_decl::OrchestrationPhaseDecl;

mod orchestration_workspace_decl;
pub use orchestration_workspace_decl::OrchestrationWorkspaceDecl;

mod type_field;
pub use type_field::TypeField;
