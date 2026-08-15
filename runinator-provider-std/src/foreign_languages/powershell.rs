use super::ForeignLanguageAdapter;

pub(super) static POWERSHELL: PowerShell = PowerShell;

pub(super) struct PowerShell;

impl ForeignLanguageAdapter for PowerShell {
    fn canonical(&self) -> &'static str {
        "powershell"
    }

    fn source_filename(&self) -> &'static str {
        "foreign.ps1"
    }

    fn runner_filename(&self) -> &'static str {
        "runinator_runner.ps1"
    }

    fn runner_source(&self) -> &'static str {
        r#"$ErrorActionPreference = "Stop"
. (Join-Path $PSScriptRoot "foreign.ps1")
if (-not (Get-Command main -CommandType Function -ErrorAction SilentlyContinue)) {
    throw "foreign code must define main(context)"
}
$context = Get-Content -Raw -LiteralPath $env:RUNINATOR_CONTEXT | ConvertFrom-Json
$result = main $context
$encoded = ConvertTo-Json -InputObject $result -Depth 100 -Compress
[IO.File]::WriteAllText($env:RUNINATOR_OUTPUT, $encoded)
"#
    }

    fn execute(&self) -> &'static str {
        "pwsh -NoLogo -NoProfile -NonInteractive -File /work/runinator_runner.ps1"
    }
}
