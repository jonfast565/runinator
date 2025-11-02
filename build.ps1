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
        'command-center.exe',
        'runinator_plugin_console.dll'
    )
    $ScriptPath = $PSScriptRoot
    $WorkspacePath = $ScriptPath
    $TargetProfile = if ($BuildProfile -eq "dev") { "debug" } else { $BuildProfile }
    $TargetDir = Join-Path -Path $WorkspacePath -ChildPath "target\$TargetProfile"
    $ArtifactsFolder = Join-Path -Path $WorkspacePath -ChildPath "target\artifacts"
    # $DbScriptsSourceDir = Join-Path -Path $WorkspacePath -ChildPath "database-scripts"
    $DbScriptsTargetDir = Join-Path -Path $WorkspacePath -ChildPath "target\artifacts\scripts"
    $TaskScriptsSourceDir = Join-Path -Path $WorkspacePath -ChildPath "task-scripts"
    $TaskScriptsTargetDir = Join-Path -Path $WorkspacePath -ChildPath "target\artifacts\task-scripts"

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

    # Write-Host "Copying database scripts from $DbScriptsSourceDir to $DbScriptsTargetDir"
    # Copy-Item -Path $DbScriptsSourceDir\* -Destination $DbScriptsTargetDir -Recurse -Force

    Write-Host "Ensuring task scripts target directory exists: $TaskScriptsTargetDir"
    Ensure-Directory -Path $TaskScriptsTargetDir

    Write-Host "Copying task scripts from $TaskScriptsSourceDir to $TaskScriptsTargetDir"
    Copy-Item -Path $TaskScriptsSourceDir\* -Destination $TaskScriptsTargetDir -Recurse -Force

    Write-Host "Build and copy operations completed successfully."

    if ($Run) {
        Write-Host "Running cargo build with profile: $BuildProfile"
        # Start both executables and capture their process objects
        Write-Host "Run runinator.exe"
        $process1 = Start-Process -FilePath "./target/artifacts/runinator.exe" -WorkingDirectory "./target/artifacts" -PassThru -NoNewWindow
        Write-Host "Run command-center.exe"
        $process2 = Start-Process -FilePath "./target/artifacts/command-center.exe" -WorkingDirectory "./target/artifacts" -PassThru

        # Wait for both processes to exit
        Wait-Process -Id $process1.Id, $process2.Id
    }

} catch {
    Write-Error "An error occurred: $_"
}
