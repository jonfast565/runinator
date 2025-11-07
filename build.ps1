param(
    [ValidateNotNullOrEmpty()]
    [string]$BuildProfile = "dev",

    [ValidateSet("Local", "Kubernetes")]
    [string]$Mode = "Local",

    [switch]$Run,
    [switch]$SkipBuild,

    [ValidateNotNullOrEmpty()]
    [string]$LocalDatabasePath = "target/artifacts/data/runinator.db",

    [ValidateNotNullOrEmpty()]
    [string]$LocalTasksFile = "runinator-importer/tasks/tasks.json",

    [ValidateRange(1024, 65535)]
    [int]$GossipBasePort = 5500,

    [switch]$IncludeCommandCenter,

    [string]$ImageRepository,
    [ValidateNotNullOrEmpty()]
    [string]$ImageTag = "local",
    [string]$KubeContext,
    [string]$KubeManifest = "runinator-stack.yaml",
    [switch]$KubeDelete,

    [ValidateNotNullOrEmpty()]
    [string]$KubeHostVolumePath = "/var/runinator",

    [ValidateSet("RabbitMQ", "Kafka")]
    [string]$KubeBrokerBackend = "RabbitMQ"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Ensure-Directory {
    param(
        [Parameter(Mandatory)]
        [string]$Path
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        [void](New-Item -ItemType Directory -Path $Path)
    }
}

function Write-Step {
    param(
        [Parameter(Mandatory)]
        [string]$Message
    )

    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Invoke-ExternalCommand {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [string]$FilePath,

        [string[]]$Arguments = @(),

        [string]$WorkingDirectory,

        [hashtable]$Environment
    )

    $displayArgs = if ($Arguments) { $Arguments -join ' ' } else { '' }
    Write-Host ">> $FilePath $displayArgs"

    # Start the process and wait for completion
    $proc = Start-Process -FilePath $FilePath `
                          -ArgumentList $Arguments `
                          -WorkingDirectory $WorkingDirectory `
                          <#-NoNewWindow#> -Wait -PassThru -WindowStyle Normal`
                          -Environment $Environment

    if ($proc.ExitCode -ne 0) {
        throw "Command '$FilePath $displayArgs' failed with exit code $($proc.ExitCode)."
    }
}


function Test-ToolAvailable {
    param(
        [Parameter(Mandatory)]
        [string]$Name
    )

    if (-not (Get-Command -Name $Name -ErrorAction SilentlyContinue)) {
        throw "Required tool '$Name' was not found on PATH."
    }
}

function Get-PluginLibraryName {
    if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
        return 'runinator_plugin_console.dll'
    }

    if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::OSX)) {
        return 'libruninator_plugin_console.dylib'
    }

    return 'libruninator_plugin_console.so'
}

function Convert-ToLinuxPath {
    param(
        [Parameter(Mandatory)]
        [string]$Path
    )

    $normalized = ($Path -replace '\\', '/').Trim()

    if ($normalized -match '^(?<drive>[A-Za-z]):(?<rest>.*)$') {
        $drive = $Matches['drive'].ToLower()
        $rest = $Matches['rest'].TrimStart('/')
        $normalized = "/mnt/$drive/$rest"
    }

    if (-not $normalized.StartsWith('/')) {
        $normalized = '/' + $normalized.TrimStart('/')
    }

    if ($normalized.Length -gt 1) {
        $normalized = $normalized.TrimEnd('/')
    }

    return $normalized
}

function Join-HostSubPath {
    param(
        [Parameter(Mandatory)]
        [string]$Root,

        [Parameter(Mandatory)]
        [string]$Child
    )

    $cleanRoot = Convert-ToLinuxPath -Path $Root
    $cleanChild = $Child.Trim('/')

    if ([string]::IsNullOrEmpty($cleanChild)) {
        return $cleanRoot
    }

    return "$cleanRoot/$cleanChild"
}

function Publish-Binaries {
    param(
        [Parameter(Mandatory)]
        [string]$TargetDir,

        [Parameter(Mandatory)]
        [string]$ArtifactsDir
    )

    Ensure-Directory -Path $ArtifactsDir

    $binaries = @(
        'runinator-scheduler.exe',
        'runinator-worker.exe',
        'runinator-importer.exe',
        'runinator-ws.exe',
        'runinator-broker.exe',
        'command-center.exe'
    )

    foreach ($binary in $binaries) {
        $source = Join-Path -Path $TargetDir -ChildPath $binary
        $destination = Join-Path -Path $ArtifactsDir -ChildPath $binary

        if (Test-Path -LiteralPath $source) {
            Copy-Item -Path $source -Destination $destination -Force
        } else {
            Write-Warning "Build artifact missing: $source"
        }
    }
}

function Prepare-LocalArtifacts {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$TargetDir,

        [Parameter(Mandatory)]
        [string]$ArtifactsDir,

        [Parameter(Mandatory)]
        [string]$PluginFileName,

        [string]$TasksFileSource
    )

    Publish-Binaries -TargetDir $TargetDir -ArtifactsDir $ArtifactsDir

    $pluginSource = Join-Path -Path $TargetDir -ChildPath $PluginFileName
    $pluginsDir = Join-Path -Path $ArtifactsDir -ChildPath 'plugins'
    Ensure-Directory -Path $pluginsDir

    if (Test-Path -LiteralPath $pluginSource) {
        Copy-Item -Path $pluginSource -Destination (Join-Path -Path $pluginsDir -ChildPath $PluginFileName) -Force
    } else {
        Write-Warning "Plugin artifact missing: $pluginSource"
    }

    $taskScriptsSource = Join-Path -Path $WorkspacePath -ChildPath 'task-scripts'
    if (Test-Path -LiteralPath $taskScriptsSource) {
        $taskScriptsTarget = Join-Path -Path $ArtifactsDir -ChildPath 'task-scripts'
        Ensure-Directory -Path $taskScriptsTarget
        Copy-Item -Path (Join-Path -Path $taskScriptsSource -ChildPath '*') -Destination $taskScriptsTarget -Recurse -Force
    } else {
        Write-Warning "Task scripts directory missing: $taskScriptsSource"
    }

    if ($TasksFileSource) {
        if (Test-Path -LiteralPath $TasksFileSource) {
            $tasksTargetDir = Join-Path -Path $ArtifactsDir -ChildPath 'tasks'
            Ensure-Directory -Path $tasksTargetDir
            Copy-Item -Path $TasksFileSource -Destination (Join-Path -Path $tasksTargetDir -ChildPath (Split-Path -Leaf $TasksFileSource)) -Force
        } else {
            Write-Warning "Tasks file not found: $TasksFileSource"
        }
    }
}

function Get-GossipArguments {
    param(
        [Parameter(Mandatory)]
        [int]$Port,

        [Parameter(Mandatory)]
        [string[]]$AllTargets
    )

    $arguments = @('--gossip-bind', '127.0.0.1', '--gossip-port', $Port)
    $otherTargets = $AllTargets | Where-Object { $_ -ne "127.0.0.1:$Port" }
    if ($otherTargets.Count -gt 0) {
        $arguments += @('--gossip-targets', ($otherTargets -join ','))
    }

    return $arguments
}

function Get-BrokerInfraManifest {
    param(
        [Parameter(Mandatory)]
        [ValidateSet('RabbitMQ', 'Kafka')]
        [string]$Backend,

        [Parameter(Mandatory)]
        [string]$HostRoot
    )

    $rabbitPath = Join-HostSubPath -Root $HostRoot -Child 'rabbitmq'
    $rabbitManifest = @"
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: runinator-rabbitmq
  namespace: runinator
spec:
  replicas: 1
  selector:
    matchLabels:
      app: runinator-rabbitmq
  template:
    metadata:
      labels:
        app: runinator-rabbitmq
    spec:
      containers:
        - name: rabbitmq
          image: rabbitmq:3.13-management
          imagePullPolicy: IfNotPresent
          ports:
            - containerPort: 5672
              name: amqp
              protocol: TCP
            - containerPort: 15672
              name: management
              protocol: TCP
          env:
            - name: RABBITMQ_DEFAULT_USER
              value: runinator
            - name: RABBITMQ_DEFAULT_PASS
              value: runinator
          volumeMounts:
            - name: rabbitmq-data
              mountPath: /var/lib/rabbitmq
      volumes:
        - name: rabbitmq-data
          hostPath:
            path: $rabbitPath
            type: DirectoryOrCreate
---
apiVersion: v1
kind: Service
metadata:
  name: runinator-rabbitmq
  namespace: runinator
spec:
  selector:
    app: runinator-rabbitmq
  ports:
    - name: amqp
      port: 5672
      targetPort: 5672
      protocol: TCP
    - name: management
      port: 15672
      targetPort: 15672
      protocol: TCP
"@

    $kafkaPath = Join-HostSubPath -Root $HostRoot -Child 'kafka'
    $kafkaManifest = @"
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: runinator-kafka
  namespace: runinator
spec:
  replicas: 1
  selector:
    matchLabels:
      app: runinator-kafka
  template:
    metadata:
      labels:
        app: runinator-kafka
    spec:
      containers:
        - name: kafka
          image: bitnami/kafka:3.7
          imagePullPolicy: IfNotPresent
          ports:
            - containerPort: 9092
              name: kafka
              protocol: TCP
            - containerPort: 9093
              name: kafka-controller
              protocol: TCP
          env:
            - name: KAFKA_ENABLE_KRAFT
              value: "yes"
            - name: KAFKA_CFG_PROCESS_ROLES
              value: "broker,controller"
            - name: KAFKA_CFG_NODE_ID
              value: "1"
            - name: KAFKA_CFG_CONTROLLER_QUORUM_VOTERS
              value: "1@runinator-kafka.runinator.svc.cluster.local:9093"
            - name: KAFKA_CFG_LISTENER_SECURITY_PROTOCOL_MAP
              value: "CONTROLLER:PLAINTEXT,PLAINTEXT:PLAINTEXT"
            - name: KAFKA_CFG_LISTENERS
              value: "PLAINTEXT://:9092,CONTROLLER://:9093"
            - name: KAFKA_CFG_ADVERTISED_LISTENERS
              value: "PLAINTEXT://runinator-kafka.runinator.svc.cluster.local:9092"
            - name: KAFKA_CFG_CONTROLLER_LISTENER_NAMES
              value: "CONTROLLER"
            - name: KAFKA_CFG_INTER_BROKER_LISTENER_NAME
              value: "PLAINTEXT"
            - name: KAFKA_CFG_AUTO_CREATE_TOPICS_ENABLE
              value: "true"
            - name: KAFKA_CFG_LOG_DIRS
              value: "/bitnami/kafka"
            - name: ALLOW_PLAINTEXT_LISTENER
              value: "yes"
          volumeMounts:
            - name: kafka-data
              mountPath: /bitnami/kafka
      volumes:
        - name: kafka-data
          hostPath:
            path: $kafkaPath
            type: DirectoryOrCreate
---
apiVersion: v1
kind: Service
metadata:
  name: runinator-kafka
  namespace: runinator
spec:
  selector:
    app: runinator-kafka
  ports:
    - name: kafka
      port: 9092
      targetPort: 9092
      protocol: TCP
    - name: kafka-controller
      port: 9093
      targetPort: 9093
      protocol: TCP
"@

    if ($Backend -eq 'Kafka') {
        return $kafkaManifest
    }

    return $rabbitManifest
}

function Start-LocalStack {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$BuildProfile,

        [Parameter(Mandatory)]
        [string]$ArtifactsDir,

        [Parameter(Mandatory)]
        [string]$LocalDatabasePath,

        [Parameter(Mandatory)]
        [int]$GossipBasePort
    )

    $pluginFileName = Get-PluginLibraryName
    $pluginPath = Join-Path -Path (Join-Path -Path $ArtifactsDir -ChildPath 'plugins') -ChildPath $pluginFileName
    if (-not (Test-Path -LiteralPath $pluginPath)) {
        Write-Warning "Plugin library not found at $pluginPath. The worker will likely fail to start."
    }

    $gossipPorts = @{
        Scheduler = $GossipBasePort + 1
        Importer  = $GossipBasePort + 2
        Web       = $GossipBasePort + 3
    }

    $allGossipTargets = $gossipPorts.Values | ForEach-Object { "127.0.0.1:$_" }

    $tasksFile = Join-Path -Path (Join-Path -Path $ArtifactsDir -ChildPath 'tasks') -ChildPath 'tasks.json'
    if (-not (Test-Path -LiteralPath $tasksFile)) {
        Write-Warning "Tasks seed file missing at $tasksFile. The importer will idle without it."
    }

    $commands = @(
        [pscustomobject]@{
            Name = 'Runinator Test Broker'
            Cmd  = './target/artifacts/runinator-broker.exe'
            Args = @()
            Environment = @{
                RUST_LOG              = 'info'
                RUNINATOR_BROKER_ADDR = '127.0.0.1:7070'
            }
        },
        [pscustomobject]@{
            Name = 'Runinator Web Service'
            Cmd  = './target/artifacts/runinator-ws.exe'
            Args = @(
                '--database', 'sqlite',
                '--sqlite-path', $LocalDatabasePath,
                '--announce-address', '127.0.0.1'
            ) + (Get-GossipArguments -Port $gossipPorts.Web -AllTargets $allGossipTargets)
            Environment = @{
                RUST_LOG = 'info'
            }
        },
        [pscustomobject]@{
            Name = 'Runinator Scheduler'
            Cmd  = './target/artifacts/runinator-scheduler.exe'
            Args = (Get-GossipArguments -Port $gossipPorts.Scheduler -AllTargets $allGossipTargets) + @(
                '--scheduler-frequency-seconds', '1',
                '--api-timeout-seconds', '30',
                '--broker-backend', 'http',
                '--broker-endpoint', 'http://127.0.0.1:7070/',
                '--broker-poll-timeout-seconds', '5'
            )
            Environment = @{
                RUST_LOG = 'info'
            }
        },
        [pscustomobject]@{
            Name = 'Runinator Worker'
            Cmd  = './target/artifacts/runinator-worker.exe'
            Args = @(
                '--dll-path', (Join-Path -Path $ArtifactsDir -ChildPath 'plugins'),
                '--broker-backend', 'http',
                '--broker-endpoint', 'http://127.0.0.1:7070/',
                '--broker-poll-timeout-seconds', '5',
                '--api-base-url', 'http://127.0.0.1:8080/'
            )
            Environment = @{
                RUST_LOG = 'info'
            }
        },
        [pscustomobject]@{
            Name = 'Runinator Importer'
            Cmd  = './target/artifacts/runinator-importer.exe'
            Args = @(
                '--tasks-file', $tasksFile,
                '--poll-interval-seconds', '30'
            ) + (Get-GossipArguments -Port $gossipPorts.Importer -AllTargets $allGossipTargets)
            Environment = @{
                RUST_LOG = 'info'
            }
        }
    )

    $processes = @()
    foreach ($command in $commands) {
        Write-Host "Starting $($command.Name)..."
        $startArgs = @{
            FilePath         = $command.Cmd
            ArgumentList     = $command.Args
            WorkingDirectory = $WorkspacePath
            PassThru         = $true
            NoNewWindow      = $true
        }

        if ($command.Environment) {
            $startArgs['Environment'] = $command.Environment
        }

        try {
            $process = Start-Process @startArgs
            $processCommand = [pscustomobject]@{
                Name      = $command.Name
                Process   = $process
                Reported  = $false
                Command   = "$($command.Cmd) $($command.Args -join ' ')"
            }
            $processes += $processCommand
            Write-Host "Started $($processCommand.Command) (PID $($process.Id))."
        } catch {
            Write-Warning "Failed to start $($command.Name): $_"
        }
    }

    if ($processes.Count -eq 0) {
        Write-Warning 'No services were started.'
        return
    }

    Write-Host ''
    Write-Host 'Runinator services are running locally. Press Ctrl+C to stop them.' -ForegroundColor Green

    try {
        while ($true) {
            $running = @($processes | Where-Object { -not $_.Process.HasExited })
            if ($running.Count -eq 0) {
                Write-Host 'All services have exited.'
                break
            }

            $completed = @($processes | Where-Object { $_.Process.HasExited -and -not $_.Reported })
            foreach ($item in $completed) {
                $item.Reported = $true
                $code = $item.Process.ExitCode
                if ($code -eq 0) {
                    Write-Host "$($item.Name) exited cleanly (code 0)."
                } else {
                    Write-Warning "$($item.Name) exited with code $code."
                }
            }

            if ($completed.Count -gt 0) {
                Write-Warning 'Stopping remaining services due to process exit.'
                break
            }

            Start-Sleep -Milliseconds 500
        }
    } finally {
        foreach ($svc in $processes) {
            if ($svc.Process -and -not $svc.Process.HasExited) {
                Write-Host "Stopping $($svc.Name) (PID $($svc.Process.Id))..."
                try {
                    $svc.Process.Kill()
                } catch {
                    Write-Warning "Failed to stop $($svc.Name): $_"
                }
            }
        }
    }
}

function Get-ImageTag {
    param(
        [Parameter(Mandatory)]
        [string]$Name,

        [string]$Repository,

        [Parameter(Mandatory)]
        [string]$Tag
    )

    if ([string]::IsNullOrWhiteSpace($Repository)) {
        return "${Name}:${Tag}"
    }

    return "${Repository}/${Name}:${Tag}"
}

function Build-ContainerImages {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [string]$Repository,

        [Parameter(Mandatory)]
        [string]$Tag
    )

    Test-ToolAvailable -Name 'docker'

    $images = @(
        @{ Name = 'runinator-scheduler'; Dockerfile = 'runinator-scheduler/Dockerfile' },
        @{ Name = 'runinator-worker';    Dockerfile = 'runinator-worker/Dockerfile' },
        @{ Name = 'runinator-importer';  Dockerfile = 'runinator-importer/Dockerfile' },
        @{ Name = 'runinator-ws';        Dockerfile = 'runinator-ws/Dockerfile' },
        @{ Name = 'runinator-broker';    Dockerfile = 'runinator-broker/Dockerfile' }
    )

    $builtImages = @{}
    foreach ($image in $images) {
        $taggedName = Get-ImageTag -Name $image.Name -Repository $Repository -Tag $Tag
        Write-Step "Building image $taggedName"

        Invoke-ExternalCommand -FilePath 'docker' -Arguments @(
            'build',
            '--file', $image.Dockerfile,
            '--tag', $taggedName,
            '.'
        ) -WorkingDirectory $WorkspacePath

        $builtImages[$image.Name] = $taggedName
    }

    return $builtImages
}

function Deploy-KubernetesStack {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$ManifestPath,

        [string]$KubeContext,

        [hashtable]$ImageMap,

        [string]$HostVolumePath = "/var/runinator",

        [ValidateSet('RabbitMQ', 'Kafka')]
        [string]$BrokerBackend = 'RabbitMQ',

        [switch]$Delete
    )

    Test-ToolAvailable -Name 'kubectl'

    $resolvedManifest = if ([System.IO.Path]::IsPathRooted($ManifestPath)) {
        $ManifestPath
    } else {
        Join-Path -Path $WorkspacePath -ChildPath $ManifestPath
    }

    if (-not (Test-Path -LiteralPath $resolvedManifest)) {
        throw "Kubernetes manifest not found at $resolvedManifest"
    }

    $tempManifest = [System.IO.Path]::ChangeExtension([System.IO.Path]::GetTempFileName(), '.yaml')
    $content = Get-Content -Path $resolvedManifest -Raw
    $linuxHostPath = Convert-ToLinuxPath -Path $HostVolumePath
    $content = $content.Replace('__HOST_VOLUME_ROOT__', $linuxHostPath)
    $content = $content.Replace('__BROKER_BACKEND__', $BrokerBackend.ToLowerInvariant())

    $brokerManifest = Get-BrokerInfraManifest -Backend $BrokerBackend -HostRoot $linuxHostPath
    if ($content.Contains('# BROKER_INFRA_PLACEHOLDER')) {
        $content = $content.Replace('# BROKER_INFRA_PLACEHOLDER', $brokerManifest)
    } else {
        $content = ($content.TrimEnd() + [Environment]::NewLine + $brokerManifest)
    }

    if ($ImageMap) {
        foreach ($entry in $ImageMap.GetEnumerator()) {
            $pattern = [regex]::Escape("your-registry/$($entry.Key):latest")
            $content = [regex]::Replace($content, $pattern, $entry.Value)
        }
    }

    Set-Content -Path $tempManifest -Value $content -Encoding utf8NoBOM

    try {
        $kubectlArgs = @()
        if ($KubeContext) {
            $kubectlArgs += @('--context', $KubeContext)
        }

        if ($Delete) {
            $kubectlArgs += @('delete', '-f', $tempManifest)
        } else {
            $kubectlArgs += @('apply', '-f', $tempManifest)
        }

        Write-Step ("kubectl " + ($kubectlArgs -join ' '))
        Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $kubectlArgs -WorkingDirectory $WorkspacePath

        if (-not $Delete) {
            $deployments = @(
                'runinator-scheduler',
                'runinator-worker',
                'runinator-importer',
                'runinator-ws',
                'runinator-broker'
            )

            foreach ($deployment in $deployments) {
                $rolloutArgs = @()
                if ($KubeContext) {
                    $rolloutArgs += @('--context', $KubeContext)
                }

                $rolloutArgs += @(
                    'rollout', 'status',
                    "deployment/$deployment",
                    '--namespace', 'runinator',
                    '--timeout', '120s'
                )

                try {
                    Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $rolloutArgs -WorkingDirectory $WorkspacePath
                } catch {
                    Write-Warning "Rollout status check failed for deployment '$deployment': $_"
                }
            }
        }
    } finally {
        if (Test-Path -LiteralPath $tempManifest) {
            Remove-Item -Path $tempManifest -ErrorAction SilentlyContinue
        }
    }
}

try {
    $workspacePath = $PSScriptRoot
    $targetProfile = if ($BuildProfile -eq 'dev') { 'debug' } else { $BuildProfile }
    $targetDir = Join-Path -Path $workspacePath -ChildPath ("target/$targetProfile")
    $artifactsDir = Join-Path -Path $workspacePath -ChildPath 'target/artifacts'

    if (-not $SkipBuild) {
        Write-Step "Building workspace with cargo profile '$BuildProfile'"
        Invoke-ExternalCommand -FilePath 'cargo' -Arguments @('build', '--profile', $BuildProfile, '--workspace') -WorkingDirectory $workspacePath
    } else {
        Write-Step 'Skipping cargo build as requested.'
    }

    $pluginFileName = Get-PluginLibraryName
    $tasksFilePath = if ([System.IO.Path]::IsPathRooted($LocalTasksFile)) {
        $LocalTasksFile
    } else {
        Join-Path -Path $workspacePath -ChildPath $LocalTasksFile
    }

    if (-not (Test-Path -LiteralPath $tasksFilePath)) {
        Write-Warning "Specified tasks file not found at $tasksFilePath"
    }

    Write-Step 'Publishing build artifacts'
    Prepare-LocalArtifacts -WorkspacePath $workspacePath -TargetDir $targetDir -ArtifactsDir $artifactsDir -PluginFileName $pluginFileName -TasksFileSource $tasksFilePath

    if (-not $Run) {
        Write-Step 'Run flag not provided. Build phase complete.'
        return
    }

    switch ($Mode) {
        'Local' {
            $dbPath = if ([System.IO.Path]::IsPathRooted($LocalDatabasePath)) {
                $LocalDatabasePath
            } else {
                Join-Path -Path $workspacePath -ChildPath $LocalDatabasePath
            }

            $dbDirectory = Split-Path -Path $dbPath -Parent
            if ($dbDirectory) {
                Ensure-Directory -Path $dbDirectory
            }

            Write-Step 'Starting local Runinator stack'
            Start-LocalStack -WorkspacePath $workspacePath -BuildProfile $BuildProfile -ArtifactsDir $artifactsDir -LocalDatabasePath $dbPath -GossipBasePort $GossipBasePort
        }
        'Kubernetes' {
            $imageMap = Build-ContainerImages -WorkspacePath $workspacePath -Repository $ImageRepository -Tag $ImageTag
            $manifestPath = if ([System.IO.Path]::IsPathRooted($KubeManifest)) {
                $KubeManifest
            } else {
                Join-Path -Path $workspacePath -ChildPath $KubeManifest
            }

            $deployArgs = @{
                WorkspacePath = $workspacePath
                ManifestPath  = $manifestPath
                KubeContext   = $KubeContext
                ImageMap      = $imageMap
                HostVolumePath = $KubeHostVolumePath
                BrokerBackend  = $KubeBrokerBackend
            }

            if ($KubeDelete) {
                $deployArgs['Delete'] = $true
                Write-Step 'Tearing down Runinator Kubernetes stack'
            } else {
                Write-Step 'Deploying Runinator to the local Kubernetes cluster'
            }

            Deploy-KubernetesStack @deployArgs
        }
    }
} catch {
    $errorRecord = $_
    $lineNumber = $errorRecord.InvocationInfo.ScriptLineNumber
    Write-Error "Error occurred at line: $lineNumber`nError message: $($errorRecord.Exception.Message)"
    exit 1
}
