use super::ForeignLanguageAdapter;

pub(super) static FSHARP: FSharp = FSharp;

pub(super) struct FSharp;

const PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="Foreign.fs" />
    <Compile Include="Runner.fs" />
  </ItemGroup>
</Project>
"#;

impl ForeignLanguageAdapter for FSharp {
    fn canonical(&self) -> &'static str {
        "fsharp"
    }

    fn source_filename(&self) -> &'static str {
        "Foreign.fs"
    }

    fn runner_filename(&self) -> &'static str {
        "Runner.fs"
    }

    fn runner_source(&self) -> &'static str {
        r#"module RuninatorRunner

open System
open System.IO
open System.Text.Json

[<EntryPoint>]
let main _ =
    use contextFile = File.OpenRead(Environment.GetEnvironmentVariable("RUNINATOR_CONTEXT"))
    let context = JsonSerializer.Deserialize<JsonElement>(contextFile)
    let result = Foreign.main context
    let options = JsonSerializerOptions()
    let resultType = if isNull result then typeof<obj> else result.GetType()
    let encoded = JsonSerializer.Serialize(result, resultType, options)
    File.WriteAllText(Environment.GetEnvironmentVariable("RUNINATOR_OUTPUT"), encoded)
    0
"#
    }

    fn additional_files(&self) -> &'static [(&'static str, &'static str)] {
        &[("runinator.fsproj", PROJECT)]
    }

    fn execute(&self) -> &'static str {
        "dotnet run --project /work/runinator.fsproj --configuration Release --artifacts-path /tmp/runinator-fsharp-artifacts"
    }
}
