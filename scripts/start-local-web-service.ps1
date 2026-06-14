param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$Args
)

$ErrorActionPreference = 'Stop'

$bootstrapBin = if ($env:RUNINATOR_BOOTSTRAP_BIN) { $env:RUNINATOR_BOOTSTRAP_BIN } else { 'runinator-bootstrap' }
$wsBin = if ($env:RUNINATOR_WS_BIN) { $env:RUNINATOR_WS_BIN } else { 'runinator-ws' }

$database = if ($env:RUNINATOR_DATABASE) { $env:RUNINATOR_DATABASE } else { 'sqlite' }
$sqlitePath = $env:RUNINATOR_SQLITE_PATH
$databaseUrl = $env:RUNINATOR_DATABASE_URL

for ($index = 0; $index -lt $Args.Count; $index++) {
    switch ($Args[$index]) {
        '--database' {
            $index++
            $database = $Args[$index]
        }
        '--sqlite-path' {
            $index++
            $sqlitePath = $Args[$index]
        }
        '--database-url' {
            $index++
            $databaseUrl = $Args[$index]
        }
    }
}

if ($database -eq 'sqlite') {
    if ([string]::IsNullOrWhiteSpace($databaseUrl)) {
        if (-not [string]::IsNullOrWhiteSpace($sqlitePath)) {
            $databaseUrl = $sqlitePath
        }
        else {
            $homeDir = if ($env:RUNINATOR_HOME) { $env:RUNINATOR_HOME } else { Join-Path -Path $HOME -ChildPath '.runinator' }
            $databaseUrl = Join-Path -Path $homeDir -ChildPath 'runinator.db'
        }
    }
    $parent = Split-Path -Path $databaseUrl -Parent
    if (-not [string]::IsNullOrWhiteSpace($parent)) {
        New-Item -ItemType Directory -Force -Path $parent | Out-Null
    }
}
elseif ([string]::IsNullOrWhiteSpace($databaseUrl)) {
    throw 'missing connection string for bootstrap: pass --database-url or set RUNINATOR_DATABASE_URL'
}

$bootstrapArgs = @(
    '--database', $database,
    '--database-url', $databaseUrl
)

if (-not [string]::IsNullOrWhiteSpace($env:RUNINATOR_AUTH_JWT_SECRET)) {
    $bootstrapArgs += @('--auth-jwt-secret', $env:RUNINATOR_AUTH_JWT_SECRET)
}
if (-not [string]::IsNullOrWhiteSpace($env:RUNINATOR_AUTH_BOOTSTRAP_ADMIN)) {
    $bootstrapArgs += @('--auth-bootstrap-admin', $env:RUNINATOR_AUTH_BOOTSTRAP_ADMIN)
}
if (-not [string]::IsNullOrWhiteSpace($env:RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY)) {
    $bootstrapArgs += @('--auth-bootstrap-service-api-key', $env:RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY)
}
if (-not [string]::IsNullOrWhiteSpace($env:RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY_NAME)) {
    $bootstrapArgs += @('--auth-bootstrap-service-api-key-name', $env:RUNINATOR_AUTH_BOOTSTRAP_SERVICE_API_KEY_NAME)
}

& $bootstrapBin @bootstrapArgs
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

& $wsBin @Args
exit $LASTEXITCODE
