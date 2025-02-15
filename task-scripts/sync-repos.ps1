# Usage: .\Sync-GitRepos.ps1 <root_folder>

if ($args.Count -eq 0) {
    Write-Host "Usage: .\Sync-GitRepos.ps1 <root_folder>"
    exit 1
}

$rootFolder = $args[0]

if (-not (Test-Path $rootFolder -PathType Container)) {
    Write-Host "Error: Folder '$rootFolder' does not exist."
    exit 1
}

Write-Host "Searching for Git repositories in: $rootFolder"

function Is-GitRepository {
    param (
        [string]$Folder
    )
    return (Test-Path (Join-Path $Folder ".git") -PathType Container)
}

function Fetch-AndSyncRepo {
    param (
        [string]$RepoPath
    )
    Write-Host "Fetching and syncing repository at: $RepoPath"
    Push-Location $RepoPath
    try {
        git fetch --all
        git pull
    }
    finally {
        Pop-Location
    }
}

function Search-AndSyncRepositories {
    param (
        [string]$RootFolder
    )
    # Use a Queue for breadth-first search
    $queue = New-Object System.Collections.Generic.Queue[string]
    $queue.Enqueue($RootFolder)

    # Array to store background jobs (similar to fibers in Ruby)
    $jobs = @()

    while ($queue.Count -gt 0) {
        $currentFolder = $queue.Dequeue()

        try {
            # Get all subdirectories excluding those starting with '.'
            $subFolders = Get-ChildItem -LiteralPath $currentFolder -Directory -Force |
                          Where-Object { $_.Name -notmatch '^\.' }
        }
        catch {
            Write-Host "Error accessing folder $currentFolder: $_"
            continue
        }

        foreach ($folder in $subFolders) {
            $path = $folder.FullName
            if (Is-GitRepository $path) {
                # Schedule the fetch and sync as a background job
                $jobs += Start-Job -ScriptBlock {
                    param($repoPath)
                    Write-Host "Fetching and syncing repository at: $repoPath"
                    Push-Location $repoPath
                    try {
                        git fetch --all
                        git pull
                    }
                    finally {
                        Pop-Location
                    }
                } -ArgumentList $path
            }
            else {
                # Enqueue non-Git directories for further searching
                $queue.Enqueue($path)
            }
        }
    }

    if ($jobs.Count -gt 0) {
        Write-Host "Waiting for fetch and sync jobs to complete..."
        $jobs | Wait-Job | Out-Null
        foreach ($job in $jobs) {
            Receive-Job $job
            Remove-Job $job
        }
    }
}

Search-AndSyncRepositories $rootFolder
Write-Host "Done!"
