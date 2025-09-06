<#
.SYNOPSIS
  Recursively find and sync all Git repositories under a root folder.

.USAGE
  .\Sync-GitRepos.ps1 -RootFolder <path> [-MaxConcurrent <int>] [-IncludeHidden]

.PARAMETER RootFolder
  The directory to search.

.PARAMETER MaxConcurrent
  Max repositories to sync in parallel (default: 6).

.PARAMETER IncludeHidden
  Include dot/directories like ".config" (default: excluded).
#>

[CmdletBinding()]
param(
  [Parameter(Mandatory=$true, Position=0)]
  [ValidateNotNullOrEmpty()]
  [string]$RootFolder,

  [ValidateRange(1, 64)]
  [int]$MaxConcurrent = 6,

  [switch]$IncludeHidden
)

$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $RootFolder -PathType Container)) {
  Write-Error "Folder '$RootFolder' does not exist or is not a directory."
  exit 1
}

# Resolve git.exe once and pass its full path into jobs so PATH isn't required.
try {
  $GitPath = (Get-Command git -CommandType Application -ErrorAction Stop).Source
} catch {
  Write-Error "Git is not available on PATH. Please install Git and try again."
  exit 1
}

Write-Host "Searching for Git repositories in: $RootFolder"
Write-Host "Max parallel syncs: $MaxConcurrent"

function Test-GitRepository {
  param([Parameter(Mandatory)][string]$Folder)

  if (Test-Path -LiteralPath (Join-Path $Folder '.git')) { return $true }

  try {
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName  = $GitPath
    $pinfo.Arguments = 'rev-parse --is-inside-work-tree'
    $pinfo.WorkingDirectory = $Folder
    $pinfo.RedirectStandardOutput = $true
    $pinfo.RedirectStandardError  = $true
    $pinfo.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($pinfo)
    $p.WaitForExit()
    if ($p.ExitCode -eq 0) {
      $out = $p.StandardOutput.ReadToEnd().Trim()
      return ($out -eq 'true')
    }
    return $false
  } catch {
    return $false
  }
}

function Get-ChildDirectories {
  param([Parameter(Mandatory)][string]$Folder)
  try {
    $dirs = Get-ChildItem -LiteralPath $Folder -Directory -Force
    if ($IncludeHidden) { return $dirs }
    return $dirs | Where-Object { $_.Name -notmatch '^\.' }
  } catch {
    Write-Warning "Cannot access '$Folder': $($_.Exception.Message)"
    @()
  }
}

# Shared sync logic; used in background jobs
$SyncScript = {
  param([string]$RepoPath, [string]$GitPathParam)

  $ErrorActionPreference = 'Continue'

  function Write-Log([string]$repo,[string]$msg,[string]$level='INFO') {
    $ts = (Get-Date).ToString('yyyy-MM-dd HH:mm:ss')
    Write-Output "[$ts] [$level] [$repo] $msg"
  }

  function Invoke-GitLogged {
    param(
      [Parameter(Mandatory)][string]$Repo,
      [Parameter(Mandatory)][string]$Exe,
      [Parameter(Mandatory)][string]$Args
    )
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe
    $psi.Arguments = $Args
    $psi.WorkingDirectory = $Repo
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError  = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $psi

    $stdoutSb = New-Object System.Text.StringBuilder
    $stderrSb = New-Object System.Text.StringBuilder

    $proc.add_OutputDataReceived({
      if ($_.Data) {
        $null = $stdoutSb.AppendLine($_.Data)
        Write-Output "[OUT] [$Repo] $($_.Data)"
      }
    })
    $proc.add_ErrorDataReceived({
      if ($_.Data) {
        $null = $stderrSb.AppendLine($_.Data)
        Write-Output "[ERR] [$Repo] $($_.Data)"
      }
    })

    [void]$proc.Start()
    $proc.BeginOutputReadLine()
    $proc.BeginErrorReadLine()
    $proc.WaitForExit()

    return [pscustomobject]@{
      ExitCode = $proc.ExitCode
      StdOut   = $stdoutSb.ToString()
      StdErr   = $stderrSb.ToString()
    }
  }

  Write-Log $RepoPath "=== Sync start ==="

  Push-Location -LiteralPath $RepoPath
  try {
    $fetch = Invoke-GitLogged -Repo $RepoPath -Exe $GitPathParam -Args 'fetch --all --tags --prune --progress'
    if ($fetch.ExitCode -ne 0) {
      Write-Log $RepoPath "git fetch failed ($($fetch.ExitCode))" 'ERROR'
      return [pscustomobject]@{ Type='RepoResult'; Path=$RepoPath; Success=$false; Message="fetch:$($fetch.ExitCode)" }
    }

    $pull = Invoke-GitLogged -Repo $RepoPath -Exe $GitPathParam -Args 'pull --ff-only --progress'
    if ($pull.ExitCode -ne 0) {
      Write-Log $RepoPath "git pull failed ($($pull.ExitCode))" 'ERROR'
      return [pscustomobject]@{ Type='RepoResult'; Path=$RepoPath; Success=$false; Message="pull:$($pull.ExitCode)" }
    }

    Write-Log $RepoPath "=== Sync OK ===" 'INFO'
    return [pscustomobject]@{ Type='RepoResult'; Path=$RepoPath; Success=$true;  Message='OK' }
  } catch {
    Write-Log $RepoPath "Exception: $($_.Exception.Message)" 'ERROR'
    return [pscustomobject]@{ Type='RepoResult'; Path=$RepoPath; Success=$false; Message='exception' }
  } finally {
    Pop-Location
  }
}

# BFS over directories, collecting repo paths
$queue = [System.Collections.Generic.Queue[string]]::new()
$queue.Enqueue((Resolve-Path -LiteralPath $RootFolder).Path)

$repos = New-Object System.Collections.Generic.List[string]

while ($queue.Count -gt 0) {
  $current = $queue.Dequeue()

  if (Test-GitRepository -Folder $current) {
    $repos.Add($current) | Out-Null
    continue
  }

  foreach ($d in Get-ChildDirectories -Folder $current) {
    $queue.Enqueue($d.FullName)
  }
}

if ($repos.Count -eq 0) {
  Write-Host "No Git repositories found under '$RootFolder'."
  exit 0
}

Write-Host "Found $($repos.Count) Git repos. Starting sync..."

$jobs = @()
foreach ($r in $repos) {
  while (($jobs | Where-Object { $_.State -eq 'Running' }).Count -ge $MaxConcurrent) {
    $done = Wait-Job -Job $jobs -Any -Timeout 5
    if ($done) { Receive-Job -Job $done -Keep | Write-Output }
  }

  $jobs += Start-Job -ScriptBlock $SyncScript -ArgumentList @($r, $GitPath)
  Write-Output ("[{0}] [INFO] [launcher] queued {1}" -f (Get-Date).ToString('yyyy-MM-dd HH:mm:ss'), $r)
}

while (($jobs | Where-Object { $_.State -in 'Running','NotStarted' }).Count -gt 0) {
  $done = Wait-Job -Job $jobs -Any -Timeout 5
  if ($done) { Receive-Job -Job $done -Keep | Write-Output }
}

# Final flush
$allOutputs = $jobs | Receive-Job -Keep
$jobs | Remove-Job | Out-Null

# Gather structured results for summary (objects tagged with Type='RepoResult')
$results = @()
foreach ($o in $allOutputs) {
  if ($null -ne $o -and $o.PSObject.Properties['Type'] -and $o.Type -eq 'RepoResult') {
    $results += $o
  }
}

if ($results.Count -eq 0) {
  Write-Host "Done! (No structured results returned; see log above.)"
} else {
  $ok = ($results | Where-Object { $_.Success }).Count
  $fail = ($results | Where-Object { -not $_.Success })
  $fc = $fail.Count
  Write-Host "Done! $ok succeeded, $fc failed."
  if ($fc -gt 0) {
    Write-Host "Failures:"
    $fail | ForEach-Object { Write-Host ("  - {0} : {1}" -f $_.Path, $_.Message) }
  }
}
