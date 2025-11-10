[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('broker', 'scheduler', 'worker', 'importer', 'ws')]
    [string]$Service,
    [string]$Workspace = (Split-Path -Parent $PSScriptRoot),
    [string]$Tag = 'dev-selftest',
    [int]$ProbeSeconds = 60,
    [switch]$SkipBuild,
    [switch]$KeepContainer
)

$serviceName = "runinator-$Service"
$dockerfile = Join-Path $Workspace "$serviceName/Dockerfile"
$imageTag = "${serviceName}:$Tag"
$container = "$serviceName-selftest"

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) {
    throw "docker CLI is required."
}

if (-not (Test-Path -LiteralPath $dockerfile)) {
    throw "Dockerfile not found at $dockerfile"
}

if (-not $SkipBuild) {
    docker build --file $dockerfile --tag $imageTag $Workspace
}

docker rm -f $container 2>$null | Out-Null

$runArgs = @('--name', $container, '--detach')
$env = @{
    RUST_LOG = 'debug'
}

$ports = @()
$cmd = @()

switch ($Service) {
    'broker' {
        $env.RUNINATOR_BROKER_ADDR = '0.0.0.0:7070'
        $ports += '127.0.0.1::7070'
    }
    'scheduler' {
        $cmd = @('--broker-backend', 'in-memory', '--gossip-targets', '')
    }
    'worker' {
        $cmd = @(
            '--broker-backend', 'in-memory',
            '--broker-consumer-id', 'self-test',
            '--api-base-url', 'http://127.0.0.1:8080/'
        )
    }
    'importer' {
        $cmd = @('--gossip-targets', '', '--poll-interval-seconds', '1')
    }
    'ws' {
        $ports += '127.0.0.1::8080'
        $cmd = @(
            '--database', 'sqlite',
            '--sqlite-path', '/tmp/runinator-selftest.db',
            '--gossip-targets', '127.0.0.1:7071',
            '--announce-address', '127.0.0.1'
        )
    }
}

$portArgs = @()
foreach ($port in $ports) { $portArgs += @('-p', $port) }

$envArgs = @()
foreach ($pair in $env.GetEnumerator()) {
    $envArgs += @('-e', "$($pair.Key)=$($pair.Value)")
}

Write-Host "Executing: docker run $($runArgs -join ' ') $($portArgs -join ' ') $($envArgs -join ' ') $imageTag $($cmd -join ' ')"
docker run @runArgs @portArgs @envArgs $imageTag @cmd | Out-Null

Start-Sleep -Seconds $ProbeSeconds
docker logs $container

docker stop $container | Out-Null
if (-not $KeepContainer) {
    docker rm $container | Out-Null
}
