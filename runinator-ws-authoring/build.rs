use std::{env, fs, path::PathBuf};

use runinator_models::{
    bundles::SettingsBundle, pipelines::PipelineBundle, workflows::WorkflowBundle,
};

fn main() {
    let workspace = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    let profile_source = workspace.join("packs/claude-availability");
    let mission_source = workspace.join("packs/ai-missions");
    let mut workflows = WorkflowBundle::default();
    let mut settings = SettingsBundle::default();
    let mut pipelines = PipelineBundle::default();

    println!("cargo:rerun-if-changed={}", profile_source.display());
    println!("cargo:rerun-if-changed={}", mission_source.display());

    let mut profile_settings = runinator_pack::source::load_pack_settings(&profile_source)
        .unwrap_or_else(|error| {
            panic!(
                "failed to compile settings in {}: {error}",
                profile_source.display()
            )
        })
        .expect("Claude starter profile");
    settings.settings.append(&mut profile_settings.settings);
    settings
        .execution_profiles
        .append(&mut profile_settings.execution_profiles);

    let mut mission_workflows = runinator_pack::source::load_workflow_bundle(&mission_source)
        .unwrap_or_else(|error| panic!("failed to compile {}: {error}", mission_source.display()));
    workflows.workflows.append(&mut mission_workflows.workflows);
    workflows.triggers.append(&mut mission_workflows.triggers);
    if let Some(mut mission_pipelines) =
        runinator_pack::source::load_pack_pipelines(&mission_source).unwrap_or_else(|error| {
            panic!(
                "failed to compile pipelines in {}: {error}",
                mission_source.display()
            )
        })
    {
        pipelines.pipelines.append(&mut mission_pipelines.pipelines);
    }

    let bytes = runinator_pack_wire::pack::PackBuilder::new(&workflows)
        .settings(Some(&settings))
        .pipelines(Some(&pipelines))
        .build()
        .expect("build embedded AI mission starter pack");
    let output = PathBuf::from(env::var("OUT_DIR").expect("build output directory"));
    fs::write(output.join("ai-missions.zip"), bytes).expect("write embedded starter pack");
}
