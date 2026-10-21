<#
.SYNOPSIS
    One command for the whole Dauntless Revived server: status | start | stop | restart | supervise.

.DESCRIPTION
    Components (in start order): the allowlist helper (public mode: opens the game UDP ports for
    players who logged in), the metagame (accounts and saves), the content server (game files for the
    friend launcher), the gateway (public mode: the TLS front door, the only public TCP port) and the
    deploy server (starts the game servers: Ramsgate on UDP 8777, the Training Dojo on 8776, hunts on
    8770-8775). Which ones run is set in data\config\server.json ("Components").

    A backup is taken before every start and after every stop: the metagame runs database migrations
    when it starts, and they must never run without a copy. A start is refused if that backup fails
    (-NoBackup overrides).

    On an installed server two scheduled tasks run "supervise" (start, then restart a crashed component
    after 5-60 s, giving up on it after 5 crashes in 10 minutes):
      "Dauntless Revived stack"      as the low-privilege "dauntless" account: everything except the
                                     allowlist helper
      "Dauntless Revived allowlist"  as SYSTEM (public mode only): just the allowlist helper, which
                                     changes one firewall rule and nothing else
    Run by an administrator, "start" therefore starts those tasks and "stop" ends them, so nothing but
    the allowlist helper ever runs with administrator rights. "restart -Only <component>" stops just
    that component and lets its supervisor start it again. On a sandbox install (no service account)
    everything runs as the current user, and -Only picks components for one call.

    Processes are recognised by their full command line (the component's entry script under this
    install's app folder) and game servers by their executable path under this install's game folder,
    so other Node programs and a Dauntless client on the same machine are never touched.

    The stack supervisor also records what the server uses, every minute (roadmap 4.12): CPU and
    memory per game server and component, the machine's CPU, RAM, disk and network, and player counts,
    in data\logs\performance\performance-<UTC date>.csv (30 days kept; counts only, no names). Set
    "PerformanceLog": false in server.json to turn it off. See Write-PerformanceLog.ps1.

.EXAMPLE
    C:\DauntlessRevived\bin\Stack.ps1 status
.EXAMPLE
    C:\DauntlessRevived\bin\Stack.ps1 restart -Only gateway
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Position = 0)][ValidateSet('status', 'start', 'stop', 'restart', 'supervise')][string]$Action = 'status',
    [string]$Root,
    [switch]$NoBackup,
    [ValidateSet('metagame', 'content', 'deploy', 'gateway', 'allowlist')][string[]]$Only,
    # Start or stop the processes here, as the current user, even when scheduled tasks are installed
    # (the installer uses it to run the metagame once before the tasks exist). Administrators only.
    [switch]$Direct
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\DauntlessServer.Common.ps1"
. "$PSScriptRoot\DauntlessServer.Performance.ps1"

$Root = Resolve-DRRoot $Root $PSScriptRoot
$P = Get-DRPaths $Root
$Cfg = Get-DRConfig $Root
$Mode = Get-DRMode $Cfg
$Bind = Get-DRConfigValue $Cfg 'BindAddress' '127.0.0.1'
$GatewayBind = Get-DRConfigValue $Cfg 'GatewayBind' '0.0.0.0'
$Fingerprint = [string](Get-DRConfigValue $Cfg 'CertFingerprint' '')
$GameDir = Get-DRConfigValue $Cfg 'GameDir' ''
$Ports = @{}
foreach ($k in 'metagame', 'deploy', 'content', 'gateway', 'allowlist') { $Ports[$k] = Get-DRPort $Cfg $k }
$Addresses = @{ metagame = $Bind; content = $Bind; deploy = '127.0.0.1'; gateway = $GatewayBind; allowlist = '127.0.0.1' }
$Configured = @(Get-DRConfigValue $Cfg 'Components' @('metagame', 'content', 'deploy'))
$UdpEnd = [int](Get-DRConfigValue $Cfg 'UdpPortEnd' 8777)
$AllowlistDir = Join-Path $P.Data 'allowlist'

$ServiceUser = Get-DRConfigValue $Cfg 'ServiceUser' ''
$Me = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$IsService = [bool]($ServiceUser -and ($Me -ieq "$env:COMPUTERNAME\$ServiceUser"))
$IsSystem = Test-DRRunningAsSystem
$ViaTask = [bool]($ServiceUser -and -not $IsService -and -not $IsSystem -and -not $Direct)
$StackTask = Get-DRConfigValue $Cfg 'StackTask' $DRNames.StackTask
$AllowlistTask = Get-DRConfigValue $Cfg 'AllowlistTask' ''
$BackupTask = Get-DRConfigValue $Cfg 'BackupTask' $DRNames.BackupTask
$PerfLog = (Get-DRConfigValue $Cfg 'PerformanceLog' $true) -ne $false
$script:SkipBackup = [bool]$NoBackup

# Which components this call acts on. The service account never runs the allowlist helper; SYSTEM runs
# nothing else.
$Enabled = @($DRComponentOrder | Where-Object { $Configured -contains $_ })
if ($ServiceUser -and ($IsService -or ($Action -eq 'supervise' -and -not $IsSystem))) { $Enabled = @($Enabled | Where-Object { $_ -ne 'allowlist' }) }
if ($IsSystem -and $ServiceUser) { $Enabled = @($Enabled | Where-Object { $_ -eq 'allowlist' }) }
if ($Only) { $Enabled = @($DRComponentOrder | Where-Object { $Only -contains $_ -and $Configured -contains $_ }) }

# Messages go to the host (shown on screen, recorded by the supervisor's transcript, and captured when
# this runs as a child process). Functions return only booleans.
function Say([string]$Text) { Write-Host $Text }
function Label([string]$C) { $DRComponents[$C].Label }
function Start-OwnerDashboard {
    if ($WhatIfPreference -or -not $ViaTask -or (Get-DRConfigValue $Cfg 'DashboardEnabled' $false) -ne $true) { return }
    try {
        Start-ScheduledTask -TaskName 'Dauntless Revived dashboard'
        if ((Get-DRConfigValue $Cfg 'DashboardOpenOnStart' $false) -eq $true) { Start-ScheduledTask -TaskName 'Dauntless Revived open dashboard' }
    } catch { Say '!! Owner dashboard task could not start; game services are unaffected.' }
}
function LogDir([string]$C) { if ($C -eq 'allowlist') { $AllowlistDir } else { $P.Logs } }
# The allowlist helper runs as SYSTEM, so its pid file must never live in a service-writable folder
# (a compromised service account could redirect a SYSTEM write with a reparse point). It goes to the
# allowlist folder, which only Administrators and SYSTEM can write; everything else keeps its pid in
# data\run. The pid file is informational only: components are found by their command line.
function PidFile([string]$C) { if ($C -eq 'allowlist') { Join-Path $AllowlistDir 'allowlist.pid' } else { Join-Path $P.Run "$C.pid" } }

# The stop flag lives in the service-writable data\run. Privileged code (SYSTEM or an administrator)
# never follows a reparse point when clearing or writing it: it removes a planted junction/symlink as
# a link, and refuses to act when the run folder itself has been turned into a junction.
function Clear-StopFlagSafe {
    if (Test-DRReparsePoint $P.Run) { Say "!! $($P.Run) is a reparse point (tampering?); not clearing the stop flag"; return }
    Remove-DRItemNoFollow $P.StopFlag
}
function Set-StopFlagSafe {
    if (Test-DRReparsePoint $P.Run) { Say "!! $($P.Run) is a reparse point (tampering?); not writing the stop flag"; return $false }
    New-Item -ItemType Directory -Force -Path $P.Run | Out-Null
    if (Test-DRReparsePoint $P.StopFlag) { Remove-DRItemNoFollow $P.StopFlag }
    Set-Content -LiteralPath $P.StopFlag -Value (Get-Date -Format o) -Encoding ASCII
    return $true
}

function Get-GatewayCheck {
    if (-not $Fingerprint) { return 'no certificate fingerprint in server.json' }
    $r = Invoke-DRHttp -Url ('https://127.0.0.1:{0}/undaunted/api/ServerStatus' -f $Ports.gateway) -Fingerprint $Fingerprint -TimeoutSec 5
    if ($r.Status -eq 200) { return 'TLS ok, certificate matches the invite fingerprint, metagame answers' }
    if ($r.Status -gt 0) { return "TLS ok, certificate matches; ServerStatus answered HTTP $($r.Status)" }
    return "not answering over TLS ($($r.Error))"
}

function Show-Status {
    $name = Get-DRConfigValue $Cfg 'ServerName' '(unnamed)'
    $commit = Get-DRConfigValue $Cfg 'Commit' 'unknown'
    "Dauntless Revived at $Root  (server '$name', $Mode mode, code $commit)"
    if ($Mode -eq 'Public') {
        'public address : {0}:{1}   certificate fingerprint {2}' -f (Get-DRConfigValue $Cfg 'PublicHost' '?'), $Ports.gateway, $Fingerprint
    }
    foreach ($c in $DRComponentOrder) {
        if ($Configured -notcontains $c) { continue }
        $procs = @(Get-DRComponentProcesses $P $c)
        if ($procs.Count) {
            foreach ($pr in $procs) {
                $l = (Get-DRListeners $pr.ProcessId) -join ', '
                if (-not $l) { $l = 'not listening yet' }
                '{0,-17}: up    pid {1,-6} {2}  {3} MB' -f (Label $c), $pr.ProcessId, $l, [math]::Round($pr.WorkingSetSize / 1MB)
            }
        } else {
            '{0,-17}: down  ({1}:{2})' -f (Label $c), $Addresses[$c], $Ports[$c]
        }
    }
    $gs = @(Get-DRGameServers $GameDir)
    "game servers     : $($gs.Count)"
    foreach ($g in $gs) {
        $port = (Get-NetUDPEndpoint -OwningProcess $g.ProcessId -ErrorAction SilentlyContinue |
            Where-Object { $_.LocalPort -ge 8700 -and $_.LocalPort -le 8799 } | Select-Object -First 1).LocalPort
        $role = 'hunt'
        if ($port -eq $UdpEnd) { $role = 'Ramsgate' } elseif ($port -eq ($UdpEnd - 1)) { $role = 'Training Dojo' }
        '   pid {0,-6} UDP {1,-5} {2,5} MB  {3}' -f $g.ProcessId, $port, [math]::Round($g.WorkingSetSize / 1MB), $role
    }
    if ($Configured -contains 'gateway' -and @(Get-DRComponentProcesses $P 'gateway').Count) {
        'gateway check    : ' + (Get-GatewayCheck)
    }
    # Text chat: the metagame's listener that the gateway forwards the game's chat connection to
    'chat             : ' + (Get-DRChatState $P $Cfg)
    if ($Configured -contains 'allowlist') {
        # The helper's own view (needs its secret: administrators and SYSTEM only; never printed).
        $alEnv = $null
        try { $alEnv = Read-DREnv $P.AllowlistEnv } catch {}
        if ($alEnv -and $alEnv['ALLOWLIST_SECRET'] -and @(Get-DRComponentProcesses $P 'allowlist').Count) {
            $r = Invoke-DRHttp -Url ('http://127.0.0.1:{0}/status' -f $Ports.allowlist) -Headers @{ 'x-allowlist-secret' = $alEnv['ALLOWLIST_SECRET'] } -TimeoutSec 5
            if ($r.Status -eq 200 -and $r.Json) {
                $last = if ($r.Json.lastApply) { "last change $($r.Json.lastApply.at) ok=$($r.Json.lastApply.ok)" } else { 'no change yet' }
                'allowlist helper : {0} player address(es) allowed, TTL {1} s, {2}{3}' -f @($r.Json.entries).Count, $r.Json.ttlSeconds, $last, $(if ($r.Json.dryRun) { ' (DRY-RUN)' } else { '' })
            } else { "allowlist helper : /status answered $($r.Status) $($r.Error)" }
        }
        if (Get-DRConfigValue $Cfg 'AllowlistDryRun' $false) {
            'allowlist rule   : DRY-RUN (the helper only logs what it would set; see data\allowlist\audit.log)'
        } else {
            $rule = Get-NetFirewallRule -Name $DRNames.AllowlistRuleName -ErrorAction SilentlyContinue
            if ($rule) {
                $addr = @(($rule | Get-NetFirewallAddressFilter).RemoteAddress)
                $count = if ($rule.Enabled -eq 'True') { "open for $($addr.Count) address(es)" } else { 'closed' }
                'allowlist rule   : {0} (UDP game ports)' -f $count
            } else { "allowlist rule   : missing ('$($DRNames.AllowlistRule)'; run the installer again)" }
        }
    }
    if ($ServiceUser) {
        foreach ($t in @($StackTask, $AllowlistTask, $BackupTask | Where-Object { $_ })) {
            $label = if ($t -eq $StackTask) { 'stack task' } elseif ($t -eq $AllowlistTask) { 'allowlist task' } else { 'backup task' }
            $task = Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue
            if ($task) {
                $info = Get-ScheduledTaskInfo -TaskName $t -ErrorAction SilentlyContinue
                $last = ''
                if ($info -and $info.LastRunTime -and $info.LastRunTime.Year -gt 2000) { $last = " (last run $($info.LastRunTime.ToString('yyyy-MM-dd HH:mm')), result $($info.LastTaskResult))" }
                '{0,-17}: {1}{2}' -f $label, $task.State, $last
            } else {
                '{0,-17}: not installed' -f $label
            }
        }
    }
    $age = Get-DRLatestBackupAge $P.Backups
    if ($null -eq $age) { 'last backup      : none yet' }
    else { 'last backup      : {0} ({1} min ago)' -f (Get-DRBackupFolders $P.Backups | Select-Object -First 1).Name, [math]::Round($age.TotalMinutes) }
    if (Test-Path -LiteralPath $P.StopFlag) { 'stop flag        : set (the supervisors do not restart anything)' }
    if (-not $PerfLog) { 'performance log  : off ("PerformanceLog": false in server.json)' }
    else {
        $pf = Join-Path (Get-DRPerfDir $P) (Get-DRPerfFileName ([datetime]::UtcNow))
        $last = $null
        if (Test-Path -LiteralPath $pf) { $last = Get-Content -LiteralPath $pf -Tail 1 -ErrorAction SilentlyContinue }
        if ($last -match '^(\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ),') { "performance log  : $pf (last sample $($Matches[1]))" }
        else { 'performance log  : no sample today yet (the stack supervisor writes one every minute)' }
    }
    if (@(Get-DRComponentProcesses $P 'metagame').Count) {
        # The metagame lists who is online to registered players only: ask with the owner key (readable
        # by administrators, SYSTEM and the service account; never printed), straight to the
        # metagame's own address.
        $h = $null
        try { $k = Read-DRAccountKey $P.OwnerKey; if ($k) { $h = @{ 'x-undaunted-user-api-key' = $k } } } catch {}
        $k = $null
        $r = Invoke-DRHttp -Url ("http://{0}:{1}/undaunted/api/ServerStatus" -f $Bind, $Ports.metagame) -Headers $h -TimeoutSec 5
        $sentKey = $null -ne $h
        $h = $null
        if ($r.Status -eq 200 -and $r.Json -and $r.Json.limited -eq $true) {
            # Not the real numbers (0 players, no game servers): the owner key was missing or refused.
            $why = if ($sentKey) { 'the server did not accept the owner key' } else { 'run this elevated so it can read the owner key' }
            'players online   : hidden (registered players only; {0})   registration: {1}' -f $why, $r.Json.registration
        } elseif ($r.Status -eq 200 -and $r.Json) {
            'players online   : {0}   game servers listed: {1}   registration: {2}' -f $r.Json.playersOnline, @($r.Json.instances).Count, $r.Json.registration
        } elseif ($r.Status -eq 404) {
            'players online   : (this metagame version has no ServerStatus)'
        } else {
            "players online   : ServerStatus did not answer ($($r.Status) $($r.Error))"
        }
    }
}

function Invoke-Backup {
    if ($script:SkipBackup) { Say 'backup skipped (-NoBackup)'; return $true }
    if (-not $PSCmdlet.ShouldProcess($P.Backups, 'Back up the database and secrets')) { return $true }
    $bk = Join-Path $PSScriptRoot 'Backup-DauntlessServer.ps1'
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $bk -Root $Root 2>&1 | Out-String; $code = $LASTEXITCODE }
    finally { $ErrorActionPreference = $old }
    if ($code -ne 0) { Say '!! backup FAILED:'; Say $out.Trim(); return $false }
    $line = $out -split "`n" | Where-Object { $_ -match '^backup:' } | Select-Object -Last 1
    if ($line) { Say $line.Trim() }
    return $true
}

function Rotate-Log([string]$Base, [string]$Dir) {
    $old = Join-Path $Dir 'old'
    foreach ($ext in '.out.log', '.err.log') {
        $f = Join-Path $Dir "$Base$ext"
        # A log path that is a reparse point was planted, not written by us: remove the link, never
        # rotate (move) through it (privileged callers must not follow it).
        if (Test-DRReparsePoint $f) { Remove-DRItemNoFollow $f; continue }
        if ((Test-Path -LiteralPath $f) -and (Get-Item -LiteralPath $f).Length -gt 0) {
            New-Item -ItemType Directory -Force -Path $old | Out-Null
            Move-Item -LiteralPath $f -Destination (Join-Path $old ('{0}.{1}{2}' -f $Base, (Get-Date -Format 'yyyyMMdd-HHmmss'), $ext)) -Force
        }
    }
    if (Test-Path -LiteralPath $old) {
        Get-ChildItem -LiteralPath $old -Filter "$Base.*" | Sort-Object Name -Descending | Select-Object -Skip 40 | Remove-Item -Force -ErrorAction SilentlyContinue
    }
}

function Wait-BindAddress {
    if ($Bind -eq '127.0.0.1') { return $true }
    for ($i = 0; $i -lt 60; $i++) {
        if (Test-DRLocalAddress $Bind) { return $true }
        if ($i -eq 0) { Say "waiting for $Bind to appear (Tailscale connecting) ..." }
        Start-Sleep -Seconds 5
    }
    Say "!! $Bind is not an address of this machine after 5 minutes. Is Tailscale connected? (tailscale status)"
    return $false
}

function Start-Component([string]$C) {
    $procs = @(Get-DRComponentProcesses $P $C)
    if ($procs.Count) { Say "$(Label $C) already up (pid $($procs[0].ProcessId))"; return $true }
    $owner = Get-DRPortOwner $Ports[$C]
    if ($owner) {
        Say "!! TCP $($Ports[$C]) is taken by $($owner.Name) (pid $($owner.Pid), $($owner.Address)). Not starting the $(Label $C)."
        return $false
    }
    $dir = Join-Path $P.App $DRComponents[$C].Dir
    $exe = Get-DRComponentExe $P $C
    $envFile = $P.($DRComponents[$C].Env)
    if (-not (Test-Path -LiteralPath $exe)) { Say "!! $(Label $C) is not installed ($exe missing)"; return $false }
    if (-not (Test-Path -LiteralPath $envFile)) { Say "!! $(Label $C) has no configuration ($envFile missing)"; return $false }
    if ($C -eq 'allowlist' -and -not (Get-DRConfigValue $Cfg 'AllowlistDryRun' $false) -and -not ($IsSystem -or (Test-DRAdmin))) {
        Say '!! the allowlist helper changes a firewall rule and must run elevated (its own scheduled task)'
        return $false
    }
    if (-not $PSCmdlet.ShouldProcess("$(Label $C) on $($Addresses[$C]):$($Ports[$C])", 'Start')) { return $true }
    $logDir = LogDir $C
    # SYSTEM (the allowlist helper) must not create or write anything under the service-writable
    # data\run; its pid lives in the allowlist folder. Only the other components use data\run.
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    if ($C -ne 'allowlist') { New-Item -ItemType Directory -Force -Path $P.Run | Out-Null }
    Rotate-Log $C $logDir
    # The .env file must win: a variable already set in this process would override its value.
    foreach ($k in @((Read-DREnv $envFile).Keys)) { [Environment]::SetEnvironmentVariable($k, $null, 'Process') }
    # Started through cmd.exe with ShellExecute (Start-Process without redirection), so the service
    # inherits no handles from this PowerShell: a caller that reads this script's output is never kept
    # waiting by the server, and cmd.exe writes the service's output to the log files.
    $out = Join-Path $logDir "$C.out.log"
    $err = Join-Path $logDir "$C.err.log"
    $verbs = (@($DRComponents[$C].Args) | ForEach-Object { "$_ " }) -join ''
    $inner = '"{0}" {1}"--env-file={2}" 1>>"{3}" 2>>"{4}"' -f $exe, $verbs, $envFile, $out, $err
    $proc = Start-Process -FilePath (Join-Path $env:WINDIR 'System32\cmd.exe') -ArgumentList ('/d /s /c "' + $inner + '"') `
        -WorkingDirectory $dir -WindowStyle Hidden -PassThru
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Milliseconds 500
        $np = @(Get-DRComponentProcesses $P $C)
        if ($np.Count) {
            $ids = @($np | ForEach-Object { [int]$_.ProcessId })
            if (Get-NetTCPConnection -LocalPort $Ports[$C] -State Listen -ErrorAction SilentlyContinue | Where-Object { $ids -contains [int]$_.OwningProcess }) {
                Set-Content -LiteralPath (PidFile $C) -Value $ids[0] -Encoding ASCII
                Say "started $(Label $C) (pid $($ids[0]), $($Addresses[$C]):$($Ports[$C]))"
                return $true
            }
        } elseif ($proc.HasExited) { break }
    }
    if (@(Get-DRComponentProcesses $P $C).Count -eq 0) { Say "!! $(Label $C) exited during start" }
    else { Say "!! $(Label $C) did not listen on port $($Ports[$C]) within 30 s" }
    Show-DRLogTail (Join-Path $logDir $C) 8
    return $false
}

function Stop-Component([string]$C) {
    foreach ($pr in Get-DRComponentProcesses $P $C) {
        if ($PSCmdlet.ShouldProcess("$(Label $C) (pid $($pr.ProcessId))", 'Stop')) {
            Stop-Process -Id $pr.ProcessId -Force -ErrorAction SilentlyContinue
            Say "stopped $(Label $C) ($($pr.ProcessId))"
        }
    }
    if (-not $WhatIfPreference) { Remove-Item -LiteralPath (PidFile $C) -Force -ErrorAction SilentlyContinue }
}

function Stop-GameServers {
    foreach ($g in Get-DRGameServers $GameDir) {
        if ($PSCmdlet.ShouldProcess("game server (pid $($g.ProcessId))", 'Stop')) {
            Stop-Process -Id $g.ProcessId -Force -ErrorAction SilentlyContinue
            Say "stopped game server ($($g.ProcessId))"
        }
    }
}

# With the server stopped nothing may stay reachable: close the game ports' allowlist rule. The helper
# rebuilds it from new logins when it starts again.
function Close-AllowlistRule {
    if ($Configured -notcontains 'allowlist' -or (Get-DRConfigValue $Cfg 'AllowlistDryRun' $false) -or (Get-DRConfigValue $Cfg 'Sandbox' $false)) { return }
    if (-not ((Test-DRAdmin) -or $IsSystem)) { return }
    $rule = Get-NetFirewallRule -Name $DRNames.AllowlistRuleName -ErrorAction SilentlyContinue
    if ($rule -and $rule.Enabled -eq 'True' -and $PSCmdlet.ShouldProcess($DRNames.AllowlistRule, 'Disable (close the game ports)')) {
        Disable-NetFirewallRule -Name $DRNames.AllowlistRuleName
        Say 'game ports closed (allowlist rule disabled)'
    }
}

function Start-Components([switch]$Supervised) {
    # The SYSTEM allowlist supervisor never touches the service-writable data\run: it is stopped through
    # its scheduled task, and the service-account stack supervisor (or an administrator) owns the flag.
    if (-not $IsSystem -and $PSCmdlet.ShouldProcess($P.StopFlag, 'Clear the stop flag')) { Clear-StopFlagSafe }
    if (-not (Wait-BindAddress)) { return $false }
    if ($Enabled -contains 'deploy' -and @(Get-DRComponentProcesses $P 'deploy').Count -eq 0) { Stop-GameServers }   # leftovers from a crash
    if ($Enabled -contains 'metagame' -and @(Get-DRComponentProcesses $P 'metagame').Count -eq 0) {
        $age = Get-DRLatestBackupAge $P.Backups
        if ($Supervised -and $null -ne $age -and $age.TotalMinutes -lt 10) {
            Say "backup: the newest backup is $([math]::Round($age.TotalMinutes)) min old; not taking another"
        } elseif (-not (Invoke-Backup)) {
            Say '!! not starting the metagame without a backup (use -NoBackup to override)'
            return $false
        }
    }
    $all = $true
    foreach ($c in $Enabled) {
        $ok = Start-Component $c
        if (-not $ok) { $all = $false; if ($c -eq 'metagame') { return $false } }
        if ($ok -and $c -eq 'metagame' -and -not $WhatIfPreference) {
            $r = Invoke-DRHttp -Url ('http://{0}:{1}/dauntless-status' -f $Bind, $Ports.metagame) -TimeoutSec 10
            if ($r.Status -ne 200) { Say "!! the metagame listens but /dauntless-status answered $($r.Status) $($r.Error)" }
        }
        if ($ok -and $c -eq 'gateway' -and -not $WhatIfPreference) { Say ('gateway check: ' + (Get-GatewayCheck)) }
        if ($ok -and $c -eq 'deploy' -and -not $WhatIfPreference) {
            $deploySettings = Read-DREnv $P.DeployEnv
            if ($deploySettings['RAMSGATE_POOL'] -ne '0') {
                Say 'Ramsgate pool ready: cities start on demand, with 20 players per instance'
                continue
            }
            $up = $false
            for ($i = 0; $i -lt 60 -and -not $up; $i++) {
                if (Get-NetUDPEndpoint -LocalPort $UdpEnd -ErrorAction SilentlyContinue) { $up = $true } else { Start-Sleep -Seconds 1 }
            }
            if ($up) { Say "Ramsgate server up on UDP $UdpEnd" }
            else { Say "Ramsgate is not on UDP $UdpEnd yet (it can take a minute; check 'Stack.ps1 status')" }
        }
    }
    return $all
}

function Stop-Components {
    if (-not $Only -and $PSCmdlet.ShouldProcess($P.StopFlag, 'Set the stop flag (the supervisors stop restarting things)')) {
        [void](Set-StopFlagSafe)
    }
    if ($ViaTask -and -not $Only) {
        foreach ($t in @($StackTask, $AllowlistTask | Where-Object { $_ })) {
            $task = Get-ScheduledTask -TaskName $t -ErrorAction SilentlyContinue
            if ($task -and $task.State -eq 'Running' -and $PSCmdlet.ShouldProcess($t, 'End the scheduled task')) {
                Stop-ScheduledTask -TaskName $t
                Say "ended the '$t' task (supervisor)"
            }
        }
    }
    # Deploy server first: it would start a new Ramsgate. Then its game servers, then the rest in reverse
    # start order (the gateway before the metagame, the allowlist helper last).
    $order = @('deploy', 'gateway', 'content', 'metagame', 'allowlist')
    $targets = if ($Only) { $Enabled } elseif ($ViaTask) { $Configured } else { $Enabled }
    foreach ($c in $order) {
        if ($targets -notcontains $c) { continue }
        Stop-Component $c
        if ($c -eq 'deploy') { Stop-GameServers }
    }
    if (-not $Only) { Close-AllowlistRule }
    if (-not $WhatIfPreference) { Start-Sleep -Seconds 2 }
}

function Start-ViaTask {
    if (-not (Test-DRAdmin)) { Stop-DR 'Run this in an elevated PowerShell (Run as administrator).' }
    $t = Get-ScheduledTask -TaskName $StackTask -ErrorAction SilentlyContinue
    if (-not $t) { Stop-DR "The scheduled task '$StackTask' does not exist. Run Install-DauntlessServer.ps1 again." }
    if (@(Get-DRComponentProcesses $P 'metagame').Count -eq 0 -and -not (Invoke-Backup)) {
        Say '!! not starting without a backup (use -NoBackup to override)'
        return
    }
    if ($PSCmdlet.ShouldProcess($P.StopFlag, 'Clear the stop flag')) { Clear-StopFlagSafe }
    if ($AllowlistTask) {
        $at = Get-ScheduledTask -TaskName $AllowlistTask -ErrorAction SilentlyContinue
        if (-not $at) { Say "!! the scheduled task '$AllowlistTask' does not exist; the game ports stay closed. Run the installer again." }
        elseif ($at.State -ne 'Running' -and $PSCmdlet.ShouldProcess($AllowlistTask, 'Start the scheduled task')) { Start-ScheduledTask -TaskName $AllowlistTask; Say "started the '$AllowlistTask' task (SYSTEM)" }
    }
    if ($t.State -eq 'Running') {
        Say "the '$StackTask' task is already running; it restarts anything that is down within a minute"
        return
    }
    if (-not $PSCmdlet.ShouldProcess($StackTask, 'Start the scheduled task')) { return }
    Start-ScheduledTask -TaskName $StackTask
    Say "started the '$StackTask' task (runs as $ServiceUser); waiting for the metagame ..."
    for ($i = 0; $i -lt 60; $i++) {
        Start-Sleep -Seconds 2
        $metaUp = @(Get-DRComponentProcesses $P 'metagame').Count -and (Get-NetTCPConnection -LocalPort $Ports.metagame -State Listen -ErrorAction SilentlyContinue)
        $gwUp = ($Configured -notcontains 'gateway') -or (@(Get-DRComponentProcesses $P 'gateway').Count -and (Get-NetTCPConnection -LocalPort $Ports.gateway -State Listen -ErrorAction SilentlyContinue))
        if ($metaUp -and $gwUp) { return }
        $t = Get-ScheduledTask -TaskName $StackTask
        if ($i -gt 3 -and $t.State -ne 'Running') {
            Say "!! the task is '$($t.State)'. See $($P.Logs)\supervisor.log"
            if ((Get-DRConfigValue $Cfg 'InteractiveSession' $false) -eq $true) {
                Say "   (interactive mode: the '$ServiceUser' account must be signed in; it signs in automatically after a reboot)"
            }
            return
        }
    }
    Say "!! the stack is not up after 2 minutes. See $($P.Logs)\supervisor.log and metagame.err.log"
}

function Invoke-Supervise {
    if ($ViaTask) { Stop-DR "supervise runs inside the '$StackTask' task as $ServiceUser (and '$AllowlistTask' as SYSTEM). Use 'Stack.ps1 start'." }
    if (-not $Enabled.Count) { Stop-DR 'nothing to supervise for this account' }
    $logDir = if ($Enabled -contains 'allowlist' -and $Enabled.Count -eq 1) { $AllowlistDir } else { $P.Logs }
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    $log = Join-Path $logDir 'supervisor.log'
    if ((Test-Path -LiteralPath $log) -and (Get-Item -LiteralPath $log).Length -gt 5MB) { Move-Item -LiteralPath $log -Destination "$log.1" -Force }
    Start-Transcript -LiteralPath $log -Append | Out-Null
    try {
        Say "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') supervisor started as $Me (pid $PID) for: $($Enabled -join ', ')"
        [void](Start-Components -Supervised)
        $history = @{}; $gaveUp = @{}
        foreach ($c in $Enabled) { $history[$c] = @() }
        # The performance log (roadmap 4.12): a sample every 4th check, about once a minute. The service
        # account (or the sandbox user) samples; the SYSTEM allowlist supervisor never does.
        $perf = $null; $tick = 0
        if ($PerfLog -and -not $IsSystem -and @($Enabled | Where-Object { $_ -ne 'allowlist' }).Count) {
            $perf = New-DRPerfState
            try { [void](Invoke-DRPerfSample -State $perf -Paths $P -Config $Cfg) } catch {}   # the CPU baseline
            Say "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') performance log: a sample every minute in $(Get-DRPerfDir $P)"
        }
        $running = $true
        while ($running) {
            Start-Sleep -Seconds 15
            # The SYSTEM allowlist supervisor does not read the service-writable stop flag (which a
            # compromised service account could redirect): an administrator ends its scheduled task.
            if (-not $IsSystem -and (Test-Path -LiteralPath $P.StopFlag)) { Say "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') stop flag set: supervisor exits"; break }
            foreach ($c in $Enabled) {
                if ($gaveUp[$c]) { continue }
                if (@(Get-DRComponentProcesses $P $c).Count) { continue }
                $now = Get-Date
                $history[$c] = @($history[$c] | Where-Object { ($now - $_).TotalMinutes -le 10 }) + $now
                $n = $history[$c].Count
                if ($n -gt 5) {
                    Say "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') !! $(Label $c) went down $n times in 10 minutes: giving up on it until the next start"
                    $gaveUp[$c] = $true
                    continue
                }
                $delay = [int][math]::Min(60, 5 * [math]::Pow(2, $n - 1))
                Say "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $(Label $c) is down: restarting in $delay s ($n time(s) in 10 min)"
                Start-Sleep -Seconds $delay
                if (-not $IsSystem -and (Test-Path -LiteralPath $P.StopFlag)) { $running = $false; break }
                if ($c -eq 'deploy') { Stop-GameServers }
                [void](Start-Component $c)
            }
            if ($running -and $perf -and ((++$tick) % 4) -eq 0) {
                $why = Invoke-DRPerfTick -State $perf -Paths $P -Config $Cfg
                if ($why) { Say "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') performance log: sample skipped ($why)" }
            }
        }
    } finally {
        Stop-Transcript | Out-Null
    }
}

try {
    if ($Direct -and $ServiceUser -and -not (Test-DRAdmin)) { Stop-DR '-Direct is for administrators.' }
    switch ($Action) {
        'status' { Show-Status }
        'start' {
            if ($ViaTask -and $Only) { Stop-DR "-Only does not apply to a start through the scheduled tasks (they start everything). Use 'restart -Only <component>'." }
            if ($ViaTask) { Start-ViaTask } else { $ok = Start-Components; if (-not $ok) { Say '!! not everything started' } }
            Start-OwnerDashboard
            if (-not $WhatIfPreference) { ''; Show-Status }
        }
        'stop' {
            if ($ViaTask -and $Only) { Stop-DR "A component stopped alone would be restarted by its supervisor. Use 'restart -Only <component>', or stop everything." }
            Stop-Components
            [void](Invoke-Backup)
            if (-not $WhatIfPreference) { ''; Show-Status }
        }
        'restart' {
            if ($ViaTask -and $Only) {
                # The supervisors start it again within a minute.
                Stop-Components
                Say "the supervisor starts $($Enabled -join ', ') again within a minute"
            } else {
                Stop-Components
                if (-not $Only) { [void](Invoke-Backup); $script:SkipBackup = $true }   # the copy was just taken
                if ($ViaTask) { Start-ViaTask } else { $ok = Start-Components; if (-not $ok) { Say '!! not everything started' } }
                Start-OwnerDashboard
            }
            if (-not $WhatIfPreference) { ''; Show-Status }
        }
        'supervise' { Invoke-Supervise }
    }
} catch {
    if ("$_" -notmatch '^DRFAIL:') { Write-Host "   FAIL  $_" -ForegroundColor Red }
    exit 1
}
