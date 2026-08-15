use super::ForeignLanguageAdapter;

pub(super) static VBNET: VbNet = VbNet;

pub(super) struct VbNet;

const PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <RootNamespace></RootNamespace>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="Foreign.vb" />
    <Compile Include="Runner.vb" />
  </ItemGroup>
</Project>
"#;

impl ForeignLanguageAdapter for VbNet {
    fn canonical(&self) -> &'static str {
        "vbnet"
    }

    fn source_filename(&self) -> &'static str {
        "Foreign.vb"
    }

    fn runner_filename(&self) -> &'static str {
        "Runner.vb"
    }

    fn runner_source(&self) -> &'static str {
        r#"Imports System
Imports System.IO
Imports System.Text.Json

Public Module RuninatorRunner
    Public Sub Main()
        Using contextFile = File.OpenRead(Environment.GetEnvironmentVariable("RUNINATOR_CONTEXT"))
            Dim context = JsonSerializer.Deserialize(Of JsonElement)(contextFile)
            Dim result = Foreign.Main(context)
            Dim encoded = JsonSerializer.Serialize(result, If(result Is Nothing, GetType(Object), result.GetType()))
            File.WriteAllText(Environment.GetEnvironmentVariable("RUNINATOR_OUTPUT"), encoded)
        End Using
    End Sub
End Module
"#
    }

    fn additional_files(&self) -> &'static [(&'static str, &'static str)] {
        &[("runinator.vbproj", PROJECT)]
    }

    fn execute(&self) -> &'static str {
        "dotnet run --project /work/runinator.vbproj --configuration Release --artifacts-path /tmp/runinator-vbnet-artifacts"
    }
}
