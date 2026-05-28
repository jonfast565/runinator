param(
    [ValidateNotNullOrEmpty()]
    [string]$BuildProfile = "dev",

    [ValidateSet("Local", "Kubernetes")]
    [string]$Mode = "Kubernetes",

    [switch]$Run,
    [switch]$SkipBuild,
    [switch]$DeployKube,

    [ValidateNotNullOrEmpty()]
    [string]$LocalDatabasePath = (Join-Path -Path $HOME -ChildPath ".runinator/runinator.db"),

    [ValidateNotNullOrEmpty()]
    [string]$LocalWorkflowsFile = (Join-Path -Path $HOME -ChildPath ".runinator/workflows/workflow-pack.json"),

    [ValidateRange(1024, 65535)]
    [int]$GossipBasePort = 5500,

    [switch]$IncludeCommandCenter,

    [string]$ImageRepository,
    [ValidateNotNullOrEmpty()]
    [string]$ImageTag = "local",
    [string]$KubeContext,
    # Path to a kustomize overlay directory (e.g. deploy/k8s/overlays/local) or
    # a raw manifest file. Defaults to the local overlay.
    [string]$KubeManifest = "deploy/k8s/overlays/local",
    [ValidateRange(1, 86400)]
    [int]$KubeImporterTimeoutSeconds = 600,
    [switch]$KubeDelete,
    # By default the postgres and rabbitmq StatefulSets are preserved if they
    # already exist in the cluster, so app rollouts do not touch the database
    # or broker. Pass this switch to apply (and potentially recreate) them.
    [switch]$KubeRecreateInfra,
    [string]$LocalRegistry = "",

    [ValidateNotNullOrEmpty()]
    [string]$WindowsTargetTriple = "x86_64-pc-windows-msvc"
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
                          -NoNewWindow -Wait -PassThru `
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

function Test-K8sResourceExists {
    param(
        [string[]]$KubeContextArgs = @(),
        [Parameter(Mandatory)] [string]$Kind,
        [Parameter(Mandatory)] [string]$Name,
        [Parameter(Mandatory)] [string]$Namespace
    )

    $getArgs = $KubeContextArgs + @('get', $Kind, $Name, '--namespace', $Namespace, '--ignore-not-found', '-o', 'name')
    $output = & kubectl @getArgs 2>$null
    if ($LASTEXITCODE -ne 0) { return $false }
    return [bool]($output | Where-Object { $_ -match '\S' })
}

function Invoke-Kubectl {
    param(
        [string[]]$KubeContextArgs = @(),
        [Parameter(Mandatory)] [string[]]$Arguments
    )

    $all = $KubeContextArgs + $Arguments
    $output = & kubectl @all 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "kubectl $($all -join ' ') failed: $output"
    }
    return ($output -join "`n")
}

function Remove-K8sStatefulSetDocs {
    param(
        [Parameter(Mandatory)] [string]$RenderedYaml,
        [switch]$SkipPostgres,
        [switch]$SkipRabbitmq
    )

    # split on lines that are exactly `---`. emit docs we want to keep.
    $docs = [System.Collections.Generic.List[string]]::new()
    $current = New-Object System.Text.StringBuilder
    foreach ($line in ($RenderedYaml -split "`r?`n")) {
        if ($line -eq '---') {
            $docs.Add($current.ToString()) | Out-Null
            $current = New-Object System.Text.StringBuilder
        } else {
            [void]$current.AppendLine($line)
        }
    }
    $docs.Add($current.ToString()) | Out-Null

    $result = New-Object System.Text.StringBuilder
    foreach ($doc in $docs) {
        if ([string]::IsNullOrWhiteSpace($doc)) { continue }
        $isSts = $doc -match '(?m)^kind:\s*StatefulSet\s*$'
        if ($SkipPostgres -and $isSts -and ($doc -match '(?m)^\s\sname:\s*runinator-postgres\s*$')) { continue }
        if ($SkipRabbitmq -and $isSts -and ($doc -match '(?m)^\s\sname:\s*runinator-rabbitmq\s*$')) { continue }
        [void]$result.AppendLine('---')
        [void]$result.Append($doc)
    }
    return $result.ToString()
}

function Invoke-KubectlApplyStdin {
    param(
        [string[]]$KubeContextArgs = @(),
        [Parameter(Mandatory)] [string]$Stdin,
        [string]$WorkingDirectory
    )

    $applyArgs = $KubeContextArgs + @('apply', '-f', '-')
    Write-Host ">> kubectl $($applyArgs -join ' ')  (filtered manifest via stdin)"
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = 'kubectl'
    foreach ($a in $applyArgs) { [void]$psi.ArgumentList.Add($a) }
    $psi.RedirectStandardInput = $true
    $psi.UseShellExecute = $false
    if ($WorkingDirectory) { $psi.WorkingDirectory = $WorkingDirectory }
    $proc = [System.Diagnostics.Process]::Start($psi)
    $proc.StandardInput.Write($Stdin)
    $proc.StandardInput.Close()
    $proc.WaitForExit()
    if ($proc.ExitCode -ne 0) {
        throw "kubectl apply -f - failed with exit code $($proc.ExitCode)."
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

function Get-ExecutableName {
    param(
        [Parameter(Mandatory)]
        [string]$Name
    )

    if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
        return "$Name.exe"
    }

    return $Name
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
        'runinator-scheduler',
        'runinator-worker',
        'runinator-importer',
        'runinator-ws',
        'runinator-broker',
        'runinator-supervisor'
    )

    foreach ($binary in $binaries) {
        $fileName = Get-ExecutableName -Name $binary
        $source = Join-Path -Path $TargetDir -ChildPath $fileName
        $destination = Join-Path -Path $ArtifactsDir -ChildPath $fileName

        if (Test-Path -LiteralPath $source) {
            Copy-Item -Path $source -Destination $destination -Force
        } else {
            Write-Warning "Build artifact missing: $source"
        }
    }
}

function Ensure-RustTarget {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$Target
    )

    Test-ToolAvailable -Name 'rustup'

    $installed = (& rustup target list --installed 2>$null) -split [Environment]::NewLine
    $isInstalled = $installed | Where-Object { $_.Trim() -eq $Target }

    if (-not $isInstalled) {
        Write-Step "Adding rustup target '$Target'"
        Invoke-ExternalCommand -FilePath 'rustup' -Arguments @('target', 'add', $Target) -WorkingDirectory $WorkspacePath
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

        [string]$WorkflowsFileSource
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

    if ($WorkflowsFileSource) {
        if (Test-Path -LiteralPath $WorkflowsFileSource) {
            $workflowsTargetDir = Join-Path -Path $ArtifactsDir -ChildPath 'workflows'
            Ensure-Directory -Path $workflowsTargetDir
            Copy-Item -Path $WorkflowsFileSource -Destination (Join-Path -Path $workflowsTargetDir -ChildPath (Split-Path -Leaf $WorkflowsFileSource)) -Force
        } else {
            Write-Warning "Workflows file not found: $WorkflowsFileSource"
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

function Write-LocalSupervisorConfig {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$ArtifactsDir,

        [Parameter(Mandatory)]
        [string]$ConfigPath,

        [Parameter(Mandatory)]
        [string]$LocalDatabasePath,

        [Parameter(Mandatory)]
        [string]$WorkflowsFile,

        [Parameter(Mandatory)]
        [int]$GossipBasePort
    )

    $gossipPorts = @{
        Scheduler = $GossipBasePort + 1
        Importer  = $GossipBasePort + 2
        Web       = $GossipBasePort + 3
    }

    $allGossipTargets = @(
        "127.0.0.1:$($gossipPorts.Scheduler)"
        "127.0.0.1:$($gossipPorts.Importer)"
        "127.0.0.1:$($gossipPorts.Web)"
    )

    $pluginFileName = Get-PluginLibraryName
    $pluginPath = Join-Path -Path (Join-Path -Path $ArtifactsDir -ChildPath 'plugins') -ChildPath $pluginFileName
    if (-not (Test-Path -LiteralPath $pluginPath)) {
        Write-Warning "Plugin library not found at $pluginPath. The worker will likely fail to start."
    }

    if (-not (Test-Path -LiteralPath $WorkflowsFile)) {
        Write-Warning "Workflows file missing at $WorkflowsFile. The importer will fail its one-shot import without it."
    }

    $commands = @(
        [ordered]@{
            name = 'Runinator Test Broker'
            command = (Join-Path -Path $ArtifactsDir -ChildPath (Get-ExecutableName -Name 'runinator-broker'))
            cwd = $WorkspacePath
            env = @{
                RUST_LOG              = 'info'
                RUNINATOR_BROKER_ADDR = '127.0.0.1:7070'
            }
        }
        [ordered]@{
            name = 'Runinator Web Service'
            command = (Join-Path -Path $ArtifactsDir -ChildPath (Get-ExecutableName -Name 'runinator-ws'))
            cwd = $WorkspacePath
            args = @(
                '--database', 'sqlite',
                '--sqlite-path', $LocalDatabasePath,
                '--announce-address', '127.0.0.1'
            ) + (Get-GossipArguments -Port $gossipPorts.Web -AllTargets $allGossipTargets)
            env = @{
                RUST_LOG = 'info'
            }
        }
        [ordered]@{
            name = 'Runinator Scheduler'
            command = (Join-Path -Path $ArtifactsDir -ChildPath (Get-ExecutableName -Name 'runinator-scheduler'))
            cwd = $WorkspacePath
            args = (Get-GossipArguments -Port $gossipPorts.Scheduler -AllTargets $allGossipTargets) + @(
                '--scheduler-frequency-seconds', '1',
                '--api-timeout-seconds', '30',
                '--broker-backend', 'http',
                '--broker-endpoint', 'http://127.0.0.1:7070/',
                '--broker-poll-timeout-seconds', '5'
            )
            env = @{
                RUST_LOG = 'info'
            }
        }
        [ordered]@{
            name = 'Runinator Worker'
            command = (Join-Path -Path $ArtifactsDir -ChildPath (Get-ExecutableName -Name 'runinator-worker'))
            cwd = $WorkspacePath
            args = @(
                '--dll-path', (Join-Path -Path $ArtifactsDir -ChildPath 'plugins'),
                '--broker-backend', 'http',
                '--broker-endpoint', 'http://127.0.0.1:7070/',
                '--broker-poll-timeout-seconds', '5',
                '--api-base-url', 'http://127.0.0.1:8080/'
            )
            env = @{
                RUST_LOG = 'info'
            }
        }
        [ordered]@{
            name = 'Runinator Importer'
            command = (Join-Path -Path $ArtifactsDir -ChildPath (Get-ExecutableName -Name 'runinator-importer'))
            cwd = $WorkspacePath
            args = @(
                '--once',
                '--workflows-file', $WorkflowsFile
            ) + (Get-GossipArguments -Port $gossipPorts.Importer -AllTargets $allGossipTargets)
            env = @{
                RUST_LOG = 'info'
            }
            restart_on_failure = $false
        }
    )

    $supervisorConfig = [ordered]@{
        state_dir = (Join-Path -Path $HOME -ChildPath '.runinator/supervisor')
        shutdown_timeout_secs = 15
        restart_delay_ms = 2000
        processes = $commands
    }

    $supervisorConfig | ConvertTo-Json -Depth 8 | Set-Content -Path $ConfigPath -Encoding utf8NoBOM
}

function Start-LocalStack {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$TargetDir,

        [Parameter(Mandatory)]
        [string]$ArtifactsDir,

        [Parameter(Mandatory)]
        [string]$LocalDatabasePath,

        [Parameter(Mandatory)]
        [string]$WorkflowsFile,

        [Parameter(Mandatory)]
        [int]$GossipBasePort
    )

    $supervisorBinary = Join-Path -Path $TargetDir -ChildPath (Get-ExecutableName -Name 'runinator-supervisor')
    if (-not (Test-Path -LiteralPath $supervisorBinary)) {
        throw "Supervisor binary was not found at $supervisorBinary. Build the workspace first."
    }

    $supervisorConfigPath = Join-Path -Path $ArtifactsDir -ChildPath 'runinator-supervisor.local.json'
    Write-LocalSupervisorConfig `
        -WorkspacePath $WorkspacePath `
        -ArtifactsDir $ArtifactsDir `
        -ConfigPath $supervisorConfigPath `
        -LocalDatabasePath $LocalDatabasePath `
        -WorkflowsFile $WorkflowsFile `
        -GossipBasePort $GossipBasePort

    Write-Host "Starting local Runinator stack via supervisor config '$supervisorConfigPath'"
    Invoke-ExternalCommand -FilePath $supervisorBinary -Arguments @('--config', $supervisorConfigPath, 'start', '--foreground') -WorkingDirectory $WorkspacePath
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

function Get-VersionedImageTag {
    param(
        [string]$RequestedTag
    )

    if (-not [string]::IsNullOrWhiteSpace($RequestedTag) -and $RequestedTag -ne 'local') {
        return $RequestedTag
    }

    $timestamp = [DateTime]::UtcNow.ToString('yyyyMMddHHmmss')
    return "kube-$timestamp"
}

function Build-ContainerImages {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [string]$Repository,

        [Parameter(Mandatory)]
        [string]$Tag,

        [switch]$PushImages
    )

    Test-ToolAvailable -Name 'docker'

    $images = @(
        @{ Name = 'runinator-scheduler'; Dockerfile = 'runinator-scheduler/Dockerfile' },
        @{ Name = 'runinator-worker';    Dockerfile = 'runinator-worker/Dockerfile' },
        @{ Name = 'runinator-importer';  Dockerfile = 'runinator-importer/Dockerfile' },
        @{ Name = 'runinator-ws';        Dockerfile = 'runinator-ws/Dockerfile' },
        @{ Name = 'runinator-migration'; Dockerfile = 'runinator-migration/Dockerfile' },
        @{ Name = 'runinator-command-center-web'; Dockerfile = 'runinator-command-center/Dockerfile'; Context = 'runinator-command-center' }
    )

    $builtImages = @{}
    foreach ($image in $images) {
        $taggedName = Get-ImageTag -Name $image.Name -Repository $Repository -Tag $Tag
        Write-Step "Building image $taggedName"

        $context = if ($image.ContainsKey('Context')) { $image.Context } else { '.' }

        Invoke-ExternalCommand -FilePath 'docker' -Arguments @(
            'build',
            '--file', $image.Dockerfile,
            '--tag', $taggedName,
            $context
        ) -WorkingDirectory $WorkspacePath

        $builtImages[$image.Name] = $taggedName

        if ($PushImages) {
            Write-Step "Pushing image $taggedName"
            Invoke-ExternalCommand -FilePath 'docker' -Arguments @('push', $taggedName) -WorkingDirectory $WorkspacePath
        }
    }

    return $builtImages
}

function New-KustomizeRenderOverlay {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$OverlayPath
    )

    $k8sRoot = [System.IO.Path]::GetFullPath((Join-Path -Path $WorkspacePath -ChildPath 'deploy/k8s'))
    $resolvedOverlay = [System.IO.Path]::GetFullPath($OverlayPath)

    if (-not $resolvedOverlay.StartsWith($k8sRoot, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Image overrides require an overlay under $k8sRoot"
    }

    $renderRoot = Join-Path -Path $WorkspacePath -ChildPath 'target/k8s-render'
    if (Test-Path -LiteralPath $renderRoot) {
        Remove-Item -Path $renderRoot -Recurse -Force
    }

    Ensure-Directory -Path $renderRoot
    Copy-Item -Path $k8sRoot -Destination $renderRoot -Recurse -Force

    $relativeOverlay = [System.IO.Path]::GetRelativePath($k8sRoot, $resolvedOverlay)
    return Join-Path -Path (Join-Path -Path $renderRoot -ChildPath 'k8s') -ChildPath $relativeOverlay
}

function Split-ImageReference {
    param(
        [Parameter(Mandatory)]
        [string]$Reference
    )

    $lastSlash = $Reference.LastIndexOf('/')
    $lastColon = $Reference.LastIndexOf(':')
    if ($lastColon -le $lastSlash) {
        throw "Image reference '$Reference' must include a tag."
    }

    return [ordered]@{
        Name = $Reference.Substring(0, $lastColon)
        Tag  = $Reference.Substring($lastColon + 1)
    }
}

function Set-KustomizeOverlayImages {
    param(
        [Parameter(Mandatory)]
        [string]$OverlayPath,

        [Parameter(Mandatory)]
        [hashtable]$ImageMap
    )

    $kustomizationPath = Join-Path -Path $OverlayPath -ChildPath 'kustomization.yaml'
    if (-not (Test-Path -LiteralPath $kustomizationPath)) {
        throw "Kustomization file not found at $kustomizationPath"
    }

    $lines = [System.Collections.Generic.List[string]]::new()
    $lines.AddRange([string[]](Get-Content -LiteralPath $kustomizationPath))
    $updated = [System.Collections.Generic.List[string]]::new()
    $seen = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)

    for ($i = 0; $i -lt $lines.Count; $i++) {
        $line = $lines[$i]
        if ($line -match '^(?<indent>\s*)-\s+name:\s+(?<name>\S+)\s*$' -and $ImageMap.ContainsKey($Matches['name'])) {
            $imageName = $Matches['name']
            $image = Split-ImageReference -Reference $ImageMap[$imageName]
            $updated.Add($line)
            $updated.Add("$($Matches['indent'])  newName: $($image.Name)")
            $updated.Add("$($Matches['indent'])  newTag: $($image.Tag)")
            [void]$seen.Add($imageName)

            while (($i + 1) -lt $lines.Count -and $lines[$i + 1] -match '^\s+new(Name|Tag):\s+') {
                $i++
            }
            continue
        }

        $updated.Add($line)
    }

    foreach ($imageName in $ImageMap.Keys) {
        if (-not $seen.Contains($imageName)) {
            throw "Kustomization at $kustomizationPath does not define image '$imageName'"
        }
    }

    Set-Content -Path $kustomizationPath -Value $updated -Encoding utf8NoBOM
}

function Deploy-KubernetesStack {
    param(
        [Parameter(Mandatory)]
        [string]$WorkspacePath,

        [Parameter(Mandatory)]
        [string]$ManifestPath,

        [string]$KubeContext,

        [ValidateRange(1, 86400)]
        [int]$ImporterTimeoutSeconds = 600,

        [hashtable]$ImageMap,

        [switch]$Delete,

        [switch]$RecreateInfra
    )

    Test-ToolAvailable -Name 'kubectl'

    $resolvedPath = if ([System.IO.Path]::IsPathRooted($ManifestPath)) {
        $ManifestPath
    } else {
        Join-Path -Path $WorkspacePath -ChildPath $ManifestPath
    }

    if (-not (Test-Path -LiteralPath $resolvedPath)) {
        throw "Kubernetes manifest or overlay not found at $resolvedPath"
    }

    # Decide between `kubectl apply -k` (overlay directory) and `-f` (raw file).
    $isOverlay = (Get-Item -LiteralPath $resolvedPath).PSIsContainer

    $applyPath = $resolvedPath

    if ($ImageMap -and $isOverlay) {
        # render from a copied overlay so image edits do not dirty the repo.
        $applyPath = New-KustomizeRenderOverlay -WorkspacePath $WorkspacePath -OverlayPath $resolvedPath
        Write-Step "Rendering image overrides into $applyPath"
        Set-KustomizeOverlayImages -OverlayPath $applyPath -ImageMap $ImageMap
    }

    $ctxArgs = @()
    if ($KubeContext) {
        $ctxArgs += @('--context', $KubeContext)
    }

    $verb = if ($Delete) { 'delete' } else { 'apply' }
    $flag = if ($isOverlay) { '-k' } else { '-f' }

    Write-Step ("kubectl " + (($ctxArgs + @($verb, $flag, $applyPath)) -join ' '))
    foreach ($staleResource in @('deployment/runinator-importer', 'job/runinator-importer', 'service/runinator-gossip')) {
        $deleteStaleArgs = $ctxArgs + @(
            'delete', $staleResource,
            '--namespace', 'runinator',
            '--ignore-not-found=true'
        )

        try {
            Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $deleteStaleArgs -WorkingDirectory $WorkspacePath
        } catch {
            Write-Warning "Importer cleanup skipped or failed for '$staleResource': $_"
        }
    }

    # decide which infra StatefulSets to preserve. on apply we skip whichever
    # ones already exist (unless -RecreateInfra was set), so re-deploys never
    # disturb the running database or broker.
    $skipPg = $false
    $skipMq = $false
    if (-not $Delete -and -not $RecreateInfra -and $isOverlay) {
        $skipPg = Test-K8sResourceExists -KubeContextArgs $ctxArgs -Kind 'statefulset' -Name 'runinator-postgres' -Namespace 'runinator'
        $skipMq = Test-K8sResourceExists -KubeContextArgs $ctxArgs -Kind 'statefulset' -Name 'runinator-rabbitmq' -Namespace 'runinator'
        if ($skipPg) { Write-Step 'Preserving existing statefulset/runinator-postgres (pass -KubeRecreateInfra to override)' }
        if ($skipMq) { Write-Step 'Preserving existing statefulset/runinator-rabbitmq (pass -KubeRecreateInfra to override)' }
    }

    if ($Delete) {
        $applyArgs = $ctxArgs + @($verb, $flag, $applyPath, '--ignore-not-found=true')
        Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $applyArgs -WorkingDirectory $WorkspacePath
    } elseif (-not $skipPg -and -not $skipMq) {
        $applyArgs = $ctxArgs + @($verb, $flag, $applyPath)
        Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $applyArgs -WorkingDirectory $WorkspacePath
    } else {
        $rendered = Invoke-Kubectl -KubeContextArgs $ctxArgs -Arguments @('kustomize', $applyPath)
        $filtered = Remove-K8sStatefulSetDocs -RenderedYaml $rendered -SkipPostgres:$skipPg -SkipRabbitmq:$skipMq
        Invoke-KubectlApplyStdin -KubeContextArgs $ctxArgs -Stdin $filtered -WorkingDirectory $WorkspacePath
    }

    if ($Delete) {
        return
    }

    $rolloutTargets = [System.Collections.Generic.List[string]]::new()
    if (-not $skipPg) { [void]$rolloutTargets.Add('statefulset/runinator-postgres') }
    if (-not $skipMq) { [void]$rolloutTargets.Add('statefulset/runinator-rabbitmq') }
    foreach ($t in @('deployment/runinator-ws', 'deployment/runinator-scheduler', 'deployment/runinator-worker', 'deployment/runinator-command-center-web')) {
        [void]$rolloutTargets.Add($t)
    }

    foreach ($target in $rolloutTargets) {
        $rolloutArgs = $ctxArgs + @(
            'rollout', 'status',
            $target,
            '--namespace', 'runinator',
            '--timeout', '120s'
        )

        try {
            Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $rolloutArgs -WorkingDirectory $WorkspacePath
        } catch {
            Write-Warning "Rollout status check failed for '$target': $_"
        }
    }

    $jobWaitArgs = $ctxArgs + @(
        'wait',
        '--for=condition=complete',
        'job/runinator-importer',
        '--namespace', 'runinator',
        '--timeout', "$($ImporterTimeoutSeconds)s"
    )

    try {
        Invoke-ExternalCommand -FilePath 'kubectl' -Arguments $jobWaitArgs -WorkingDirectory $WorkspacePath
    } catch {
        Write-Warning "Importer Job did not complete within timeout: $_"
    }
}

try {
    $workspacePath = $PSScriptRoot
    $targetProfile = if ($BuildProfile -eq 'dev') { 'debug' } else { $BuildProfile }
    $targetDir = Join-Path -Path $workspacePath -ChildPath ("target/$targetProfile")
    $artifactsDir = Join-Path -Path $workspacePath -ChildPath 'target/artifacts'

    if ($DeployKube) {
        $Mode = 'Kubernetes'
        $Run = $true
    }

    if ($Mode -eq 'Kubernetes') {
        $ImageTag = Get-VersionedImageTag -RequestedTag $ImageTag

        if ([string]::IsNullOrWhiteSpace($ImageRepository) -and -not [string]::IsNullOrWhiteSpace($LocalRegistry)) {
            $ImageRepository = $LocalRegistry.TrimEnd('/')
        }
    }

    $shouldBuildLocal = ($Mode -eq 'Local' -and -not $SkipBuild)

    if ($shouldBuildLocal) {
        if ([System.Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([System.Runtime.InteropServices.OSPlatform]::Windows)) {
            Ensure-RustTarget -WorkspacePath $workspacePath -Target $WindowsTargetTriple
        }
        Write-Step "Building workspace with cargo profile '$BuildProfile'"
        Invoke-ExternalCommand -FilePath 'cargo' -Arguments @('build', '--profile', $BuildProfile, '--workspace') -WorkingDirectory $workspacePath
    } elseif ($Mode -eq 'Local') {
        Write-Step 'Skipping cargo build as requested.'
    } else {
        Write-Step 'Skipping local cargo build; Kubernetes container images build artifacts internally.'
    }

    if ($Mode -eq 'Local') {
        $pluginFileName = Get-PluginLibraryName
        $workflowsFilePath = if ([System.IO.Path]::IsPathRooted($LocalWorkflowsFile)) {
            $LocalWorkflowsFile
        } else {
            Join-Path -Path $workspacePath -ChildPath $LocalWorkflowsFile
        }

        $workflowsDirectory = Split-Path -Path $workflowsFilePath -Parent
        if ($workflowsDirectory) {
            Ensure-Directory -Path $workflowsDirectory
        }

        if (-not (Test-Path -LiteralPath $workflowsFilePath)) {
            Write-Warning "Specified workflows file not found at $workflowsFilePath"
        }

        Write-Step 'Publishing build artifacts'
        Prepare-LocalArtifacts -WorkspacePath $workspacePath -TargetDir $targetDir -ArtifactsDir $artifactsDir -PluginFileName $pluginFileName -WorkflowsFileSource $workflowsFilePath
    } else {
        Write-Step 'Skipping local artifact publication for Kubernetes mode.'
    }

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
            Start-LocalStack -WorkspacePath $workspacePath -TargetDir $targetDir -ArtifactsDir $artifactsDir -LocalDatabasePath $dbPath -WorkflowsFile $workflowsFilePath -GossipBasePort $GossipBasePort
        }
        'Kubernetes' {
            $imageMap = $null
            if (-not $KubeDelete) {
                $shouldPushImages = -not [string]::IsNullOrWhiteSpace($ImageRepository)
                $imageMap = Build-ContainerImages `
                    -WorkspacePath $workspacePath `
                    -Repository $ImageRepository `
                    -Tag $ImageTag `
                    -PushImages:$shouldPushImages
            }

            $manifestPath = if ([System.IO.Path]::IsPathRooted($KubeManifest)) {
                $KubeManifest
            } else {
                Join-Path -Path $workspacePath -ChildPath $KubeManifest
            }

            $deployArgs = @{
                WorkspacePath = $workspacePath
                ManifestPath  = $manifestPath
                KubeContext   = $KubeContext
                ImporterTimeoutSeconds = $KubeImporterTimeoutSeconds
                ImageMap      = $imageMap
            }

            if ($KubeDelete) {
                $deployArgs['Delete'] = $true
                Write-Step 'Tearing down Runinator Kubernetes stack'
            } else {
                Write-Step 'Deploying Runinator to the local Kubernetes cluster'
            }

            if ($KubeRecreateInfra) {
                $deployArgs['RecreateInfra'] = $true
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
