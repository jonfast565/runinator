use super::ForeignLanguageAdapter;

pub(super) static CSHARP: CSharp = CSharp;

pub(super) struct CSharp;

const PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
  </PropertyGroup>
</Project>
"#;

impl ForeignLanguageAdapter for CSharp {
    fn canonical(&self) -> &'static str {
        "csharp"
    }

    fn source_filename(&self) -> &'static str {
        "Foreign.cs"
    }

    fn runner_filename(&self) -> &'static str {
        "Program.cs"
    }

    fn runner_source(&self) -> &'static str {
        r#"using System.Text.Json;

using var contextFile = File.OpenRead(Environment.GetEnvironmentVariable("RUNINATOR_CONTEXT")!);
var context = await JsonSerializer.DeserializeAsync<JsonElement>(contextFile);
var result = Foreign.Main(context);
await File.WriteAllTextAsync(
    Environment.GetEnvironmentVariable("RUNINATOR_OUTPUT")!,
    JsonSerializer.Serialize(result, result?.GetType() ?? typeof(object)));
"#
    }

    fn additional_files(&self) -> &'static [(&'static str, &'static str)] {
        &[("runinator.csproj", PROJECT)]
    }

    fn execute(&self) -> &'static str {
        "dotnet run --project /work/runinator.csproj --configuration Release --artifacts-path /tmp/runinator-csharp-artifacts"
    }
}
