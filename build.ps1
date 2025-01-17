param(
    [string]$BuildProfile = "dev",
    [switch]$Run = $false
)

function Ensure-Directory {
    param(
        [string]$Path
    )
    if (-Not (Test-Path -Path $Path)) {
        New-Item -ItemType Directory -Path $Path | Out-Null
    }
}

try {
    $FilesToCopy = @(
        'runinator.exe', 
        'runinator_plugin_console.dll'
    )
    $ScriptPath = $PSScriptRoot
    $WorkspacePath = $ScriptPath
    $TargetProfile = if ($BuildProfile -eq "dev") { "debug" } else { $BuildProfile }
    $TargetDir = Join-Path -Path $WorkspacePath -ChildPath "target\$TargetProfile"
    $ArtifactsFolder = Join-Path -Path $WorkspacePath -ChildPath "target\artifacts"
    $DbScriptsSourceDir = Join-Path -Path $WorkspacePath -ChildPath "database-scripts"
    $DbScriptsTargetDir = Join-Path -Path $WorkspacePath -ChildPath "target\artifacts\scripts"

    Write-Host "Switching to workspace: $WorkspacePath"
    Set-Location -Path $WorkspacePath

    Write-Host "Running cargo build with profile: $BuildProfile"
    cargo build --profile $BuildProfile --workspace

    Write-Host "Ensuring artifacts folder exists: $ArtifactsFolder"
    Ensure-Directory -Path $ArtifactsFolder

    Write-Host "Copying specified files to artifacts folder"
    foreach ($File in $FilesToCopy) {
        $SourcePath = Join-Path -Path $TargetDir -ChildPath $File
        if (Test-Path -Path $SourcePath) {
            $DestinationPath = Join-Path -Path $ArtifactsFolder -ChildPath (Split-Path -Leaf $File)
            Copy-Item -Path $SourcePath -Destination $DestinationPath -Force
        } else {
            Write-Warning "File not found: $SourcePath"
        }
    }

    Write-Host "Ensuring database scripts target directory exists: $DbScriptsTargetDir"
    Ensure-Directory -Path $DbScriptsTargetDir

    Write-Host "Copying database scripts from $DbScriptsSourceDir to $DbScriptsTargetDir"
    Copy-Item -Path $DbScriptsSourceDir\* -Destination $DbScriptsTargetDir -Recurse -Force

    Write-Host "Build and copy operations completed successfully."

    if ($Run) {
        Write-Host "Running cargo build with profile: $BuildProfile"
        # Set-Location
        .\target\artifacts\runinator.exe
    }

} catch {
    Write-Error "An error occurred: $_"
}
