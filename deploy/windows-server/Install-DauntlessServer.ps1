<#
.SYNOPSIS
    Installs a Dauntless Revived server on Windows Server 2019 (or newer). Public mode: friends connect
    over the internet through one TLS gateway port. Private mode: friends connect over Tailscale.

.DESCRIPTION
    One run sets up everything; running it again repairs or completes an install and changes nothing
    that is already right. -WhatIf shows every change without making it. No key, token, password or
    .env value is ever printed.

      1. Preflight: administrator, 64-bit Windows Server 2019 (build 17763) or newer with the Desktop
         Experience (the game servers are the game's own exe and import DirectX/dxgi), 8 GB RAM or more,
         a 40 GB disk budget for the game zip, the extracted game and the server, free ports.
      2. Prerequisites: Node.js LTS (pinned version, SHA-256 checked against the pin and against that
         release's SHASUMS256.txt), the Microsoft Visual C++ 2015-2022 x64 runtime and the DirectX June
         2010 runtime (both checked by their Microsoft Authenticode signature). Private mode also
         installs Tailscale (pinned SHA-256) and signs in.
      3. Server code: from an uploaded source zip (-SourceZip, what Deploy-Remote.ps1 sends), a folder
         (-SourceDir), the checkout this script runs in, or GitHub for a pinned ref (-Ref). Built with
         npm ci + npm run build in <root>\app, never in the source.
      4. Game files: the verified 1.4.4 zip (SHA-256 556B9A64...BC6D; -GameZip, or -GameZipUrl which
         downloads with resume) extracted with tar.exe, every file checked against the content manifest,
         and the two server DLLs (pinned hashes).
      5. A local low-privilege account "dauntless" that runs the stack, with its game config (Game.ini
         endpoints, Engine.ini memory caps and chat override). Its password is random and never shown;
         every run of this script sets a new one, and Task Scheduler keeps it, encrypted, for the
         account's tasks (some Server 2019 images refuse password-less S4U tasks for non-admins).
      6. Public mode: a self-signed TLS certificate for the gateway (the gateway's make-cert tool; ECDSA,
         10 years, SAN = -PublicHost). Its SHA-256 fingerprint goes into every invite, so keep it: a
         new certificate invalidates all invites. No Windows certificate store is touched.
      7. Configuration: .env files with fresh RS256 signing keys, a game-server key, the gateway secret
         and the allowlist secret (or the ones from -RestoreFrom). Public mode: metagame, content server,
         deploy server and allowlist helper listen on 127.0.0.1 only; the gateway is the only public
         listener. Secrets are readable only by Administrators, SYSTEM and the service account (the
         allowlist secret file only by Administrators and SYSTEM).
      8. The database and the owner (admin) account, whose key goes to <root>\data\keys\owner.key and is
         never printed; or the database and keys from a backup (-RestoreFrom).
      9. Windows Firewall, public mode: inbound TCP <GatewayPort> from anywhere (node.exe only); UDP
         8770-8777 only through the rule "Dauntless Revived game ports (allowlist)" (created disabled; the
         allowlist helper fills in the addresses of players who logged in); SSH 22 kept (password login
         turned off when installed over SSH); RDP 3389 limited to -AdminIp, or, without -AdminIp, any
         internet-open RDP rule disabled (-KeepRdpOpen keeps it); every profile on with inbound blocked
         by default.
         Private mode: TCP and UDP only from Tailscale addresses.
     10. Scheduled tasks: the stack at startup as "dauntless" (metagame, content server, gateway, deploy
         server; supervised), public mode also the allowlist helper at startup as SYSTEM with highest
         privileges (it only changes its one firewall rule), and an hourly backup.

    Session 0: at startup the stack runs without a desktop (session 0). The game servers open a console
    window (AllocConsole); that works without a desktop in principle, but it is untested. If the game
    servers do not start that way, use -InteractiveSession: the "dauntless" account then signs in
    automatically at boot (its password is kept as an LSA secret, as Windows' own auto-logon does), the
    stack starts at that logon, and the session is locked straight away.

    -Sandbox is for testing the kit on a development PC: it skips every system-level step (Node,
    runtimes, Tailscale, firewall, accounts, tasks), listens on 127.0.0.1 only (the gateway too), uses
    spare ports (metagame 62000, deploy 62001, content 62002, allowlist 62005, gateway 62443), runs the
    allowlist helper in dry-run mode, never starts a deploy server or game server, runs as the current
    user, needs a stand-in game folder (-GameDir) and an explicit -InstallRoot.

.PARAMETER Mode
    Public (default for a new install): friends connect to -PublicHost:-GatewayPort with a v2 invite.
    Private: friends connect over Tailscale with a v1 invite. A re-run keeps the installed mode unless
    -Mode is given.
.PARAMETER GameZip
    The verified Dauntless 1.4.4 zip. Not needed when the game is already installed under the root.
.PARAMETER GameZipUrl
    Where the server downloads the zip itself (resumable; checked against the pinned SHA-256).
.PARAMETER GameDir
    An already extracted game folder (the one that contains Archon\) to use in place instead of a zip.
.PARAMETER PublicHost
    Public mode: the public IPv4 address or DNS name friends connect to. Defaults to the one public
    IPv4 address on this machine's network adapters (a VPS behind 1:1 NAT needs it given).
.PARAMETER GatewayPort
    Public mode: the gateway's TCP port (default 443).
.PARAMETER AdminIp
    Public mode: the address(es) allowed to use Remote Desktop (IPv4 or IPv4/prefix). Without it, any
    RDP rule open to the whole internet is disabled (use -KeepRdpOpen to keep it).
.PARAMETER KeepRdpOpen
    Public mode without -AdminIp: leave internet-open Remote Desktop rules open instead of disabling them.
.PARAMETER RestoreFrom
    A backup folder (<backups>\yyyy-MM-dd_HHmmss, from Backup-DauntlessServer.ps1 or the original host's
    backup.ps1): brings the database, signing keys, game-server key, account keys and (from a server
    backup) the gateway certificate along, so every account, save and invite moves to this server.
.PARAMETER SourceZip
    A zip of the server source (git archive), as uploaded by Deploy-Remote.ps1. -SourceCommit names its
    commit for the version reports.
.PARAMETER OwnerName
    Username of the owner (admin) account: 3-16 letters, digits or _. Not used with -RestoreFrom (the
    backup has the accounts).
.PARAMETER Chat
    On or Off: the in-game text chat (roadmap 3.10), the metagame's chat listener on 127.0.0.1:61099
    that the gateway forwards the game's chat connection to. Public mode only for now; no firewall
    rule is needed. Kept in server.json ("Chat"); a re-run without -Chat keeps it, a new install is
    On in public mode. Set-Chat.ps1 -On / -Off switches it later without the installer.

.EXAMPLE
    .\Install-DauntlessServer.ps1 -Mode Public -GameZip D:\BaseGame144.zip -PublicHost 203.0.113.7 -AdminIp 198.51.100.20 -RestoreFrom D:\backups\2026-10-01_200000
.EXAMPLE
    .\Install-DauntlessServer.ps1 -Mode Public -GameZipUrl https://example.org/BaseGame144.zip -OwnerName Slayer -WhatIf
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [ValidateSet('Public', 'Private')][string]$Mode,
    [string]$GameZip,
    [string]$GameZipUrl,
    [string]$GameDir,
    [string]$InstallRoot = 'C:\DauntlessRevived',
    [string]$ServerName = 'Dauntless Revived',
    [string]$PublicHost,
    [ValidateRange(1, 65535)][int]$GatewayPort = 443,
    [string[]]$AdminIp,
    [string]$OwnerName,
    # Public mode without -AdminIp: by default the internet-open Remote Desktop rules are turned off
    # (key-only SSH is the intended administration path). -KeepRdpOpen leaves them open.
    [switch]$KeepRdpOpen,
    [string]$RestoreFrom,
    [string]$Ref,
    [string]$SourceDir,
    [string]$SourceZip,
    [string]$SourceCommit,
    [switch]$InteractiveSession,
    [ValidateSet('On', 'Off')][string]$Chat,
    [switch]$NewCertificate,
    [switch]$Sandbox,
    [switch]$NoStart,
    # Private mode (Tailscale)
    [string]$TailscaleAuthKey,
    [string]$TailscaleHostname = 'dauntless-server',
    [string]$TailscaleShareUrl,
    [string]$AdvertiseHost,
    # Ports (defaults: 61000, 61001, 61002, 61005; sandbox 62000, 62001, 62002, 62005)
    [ValidateRange(1024, 65535)][int]$MetagamePort,
    [ValidateRange(1024, 65535)][int]$DeployPort,
    [ValidateRange(1024, 65535)][int]$ContentPort,
    [ValidateRange(1024, 65535)][int]$AllowlistPort,
    # Development: a manifest for a stand-in game folder (sandbox), and the allowlist helper in dry-run.
    [string]$ContentManifest,
    [switch]$AllowlistDryRun
)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\DauntlessServer.Common.ps1"

$IsWhatIf = [bool]$WhatIfPreference
$FirewallNeedsRestart = $false
$UdpBegin = $DRUdpBegin
$UdpEnd = $DRUdpEnd
$ServiceUser = $DRNames.ServiceUser

function Test-Do([string]$Target, [string]$Action) { return $PSCmdlet.ShouldProcess($Target, $Action) }

# Runs one of this kit's scripts in a child PowerShell and shows its output.
function Invoke-KitScript([string]$Name, [string[]]$Arguments) {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try {
        $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $P.Bin $Name) @Arguments 2>&1 | Out-String
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $old }
    foreach ($l in ($out -split "`r?`n")) { if ($l.Trim()) { Write-DRInfo $l } }
    return $code
}

function Invoke-DbTool([string[]]$DbArgs, [string]$LogName) {
    $code = Invoke-DRNative -FilePath $NodeExe -Arguments (@((Join-Path $P.Bin 'lib\dr-db.js')) + $DbArgs) -WorkingDirectory $P.Root `
        -LogBase (Join-Path $P.Logs "install\$LogName")
    $msg = (Get-Content -LiteralPath (Join-Path $P.Logs "install\$LogName.out.log") -ErrorAction SilentlyContinue | Select-Object -Last 1)
    $err = (Get-Content -LiteralPath (Join-Path $P.Logs "install\$LogName.err.log") -ErrorAction SilentlyContinue | Where-Object { $_.Trim() } | Select-Object -Last 1)
    return [pscustomobject]@{ Code = $code; Message = $msg; Error = $err }
}

function New-DRPassword {
    # 32 characters from all four character classes (meets any complexity policy). Never shown or
    # logged; Windows keeps it only where the account needs it (Task Scheduler for its tasks, and the
    # auto-logon LSA secret with -InteractiveSession).
    $sets = @('ABCDEFGHJKLMNPQRSTUVWXYZ', 'abcdefghijkmnopqrstuvwxyz', '23456789', '!#%+-.=?@_~')
    $all = -join $sets
    $chars = New-Object System.Collections.Generic.List[char]
    foreach ($s in $sets) { $chars.Add($s[(Get-DRRandomBytes 1)[0] % $s.Length]) }
    while ($chars.Count -lt 32) {
        $b = (Get-DRRandomBytes 1)[0]
        if ($b -lt (256 - (256 % $all.Length))) { $chars.Add($all[$b % $all.Length]) }
    }
    $sec = New-Object Security.SecureString
    foreach ($i in (0..($chars.Count - 1) | Sort-Object { [BitConverter]::ToUInt32((Get-DRRandomBytes 4), 0) })) { $sec.AppendChar($chars[$i]) }
    $chars.Clear()
    $sec.MakeReadOnly()
    return $sec
}

# LSA: the "Log on as a batch job" right (scheduled tasks that run without a logon need it) and the
# auto-logon password secret (-InteractiveSession). Compiled only when needed.
function Initialize-DRLsa {
    if ('DRKit.Lsa' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace DRKit {
    public static class Lsa {
        [StructLayout(LayoutKind.Sequential)] struct UNICODE_STRING { public ushort Length; public ushort MaximumLength; public IntPtr Buffer; }
        [StructLayout(LayoutKind.Sequential)] struct OBJECT_ATTRIBUTES { public int Length; public IntPtr RootDirectory; public IntPtr ObjectName; public uint Attributes; public IntPtr SecurityDescriptor; public IntPtr SecurityQualityOfService; }
        [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr SystemName, ref OBJECT_ATTRIBUTES Attributes, uint Access, out IntPtr Handle);
        [DllImport("advapi32.dll")] static extern uint LsaAddAccountRights(IntPtr Handle, byte[] Sid, UNICODE_STRING[] Rights, uint Count);
        [DllImport("advapi32.dll", EntryPoint = "LsaStorePrivateData")] static extern uint LsaStore(IntPtr Handle, ref UNICODE_STRING Key, ref UNICODE_STRING Data);
        [DllImport("advapi32.dll", EntryPoint = "LsaStorePrivateData")] static extern uint LsaDelete(IntPtr Handle, ref UNICODE_STRING Key, IntPtr Data);
        [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr Handle);
        [DllImport("advapi32.dll")] static extern int LsaNtStatusToWinError(uint Status);
        const uint POLICY_ALL_ACCESS = 0x000F0FFF;
        static UNICODE_STRING Str(string s) {
            UNICODE_STRING u = new UNICODE_STRING();
            u.Buffer = Marshal.StringToHGlobalUni(s);
            u.Length = (ushort)(s.Length * 2);
            u.MaximumLength = (ushort)(s.Length * 2 + 2);
            return u;
        }
        static void Free(UNICODE_STRING u) {
            if (u.Buffer != IntPtr.Zero) {
                for (int i = 0; i < u.MaximumLength; i++) Marshal.WriteByte(u.Buffer, i, 0);
                Marshal.FreeHGlobal(u.Buffer);
            }
        }
        static IntPtr Open() {
            OBJECT_ATTRIBUTES a = new OBJECT_ATTRIBUTES();
            a.Length = Marshal.SizeOf(typeof(OBJECT_ATTRIBUTES));
            IntPtr h;
            uint r = LsaOpenPolicy(IntPtr.Zero, ref a, POLICY_ALL_ACCESS, out h);
            if (r != 0) throw new Win32Exception(LsaNtStatusToWinError(r));
            return h;
        }
        public static void AddRight(string sid, string right) {
            System.Security.Principal.SecurityIdentifier s = new System.Security.Principal.SecurityIdentifier(sid);
            byte[] b = new byte[s.BinaryLength];
            s.GetBinaryForm(b, 0);
            UNICODE_STRING[] rights = new UNICODE_STRING[] { Str(right) };
            IntPtr h = Open();
            try {
                uint r = LsaAddAccountRights(h, b, rights, 1);
                if (r != 0) throw new Win32Exception(LsaNtStatusToWinError(r));
            } finally { Free(rights[0]); LsaClose(h); }
        }
        public static void StoreSecret(string name, IntPtr bstr) {
            UNICODE_STRING key = Str(name);
            IntPtr h = Open();
            try {
                uint r;
                if (bstr == IntPtr.Zero) {
                    r = LsaDelete(h, ref key, IntPtr.Zero);
                } else {
                    int len = Marshal.ReadInt32(bstr, -4);
                    UNICODE_STRING data = new UNICODE_STRING();
                    data.Buffer = bstr;
                    data.Length = (ushort)len;
                    data.MaximumLength = (ushort)len;
                    r = LsaStore(h, ref key, ref data);
                    if (r != 0) throw new Win32Exception(LsaNtStatusToWinError(r));
                }
            } finally { Free(key); LsaClose(h); }
        }
    }
}
'@
}

function Set-DRAutoLogon([Security.SecureString]$Password) {
    Initialize-DRLsa
    $wl = 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon'
    if ($Password) {
        $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Password)
        try { [DRKit.Lsa]::StoreSecret('DefaultPassword', $bstr) } finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr) }
        Remove-ItemProperty -Path $wl -Name DefaultPassword -ErrorAction SilentlyContinue   # never in plain text
        Set-ItemProperty -Path $wl -Name AutoAdminLogon -Value '1' -Type String
        Set-ItemProperty -Path $wl -Name DefaultUserName -Value $ServiceUser -Type String
        Set-ItemProperty -Path $wl -Name DefaultDomainName -Value $env:COMPUTERNAME -Type String
    } else {
        [DRKit.Lsa]::StoreSecret('DefaultPassword', [IntPtr]::Zero)
        Set-ItemProperty -Path $wl -Name AutoAdminLogon -Value '0' -Type String
    }
}

# True when the firewall that is actually in effect (local settings merged with any policy) is on and
# blocks inbound by default on every profile.
function Test-DRFirewallEffective {
    $p = @(Get-NetFirewallProfile -PolicyStore ActiveStore -ErrorAction SilentlyContinue)
    return ($p.Count -gt 0 -and -not @($p | Where-Object { -not $_.Enabled -or "$($_.DefaultInboundAction)" -ne 'Block' }).Count)
}

# Registers a task that runs as the service account with its password stored by Task Scheduler (logon
# type Password, kept encrypted in LSA). S4U ("do not store password") would need no password, but
# some Server 2019 images refuse S4U for any account that is not an administrator ("Access is
# denied", even when SYSTEM registers it), so the installer sets a fresh random password, hands it to
# Task Scheduler and forgets it.
function Register-DRServiceTask([string]$TaskName, $Action, $Trigger, $Settings, [string]$Description, [Security.SecureString]$Password) {
    $bstr = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Password)
    try {
        $a = @{ TaskName = $TaskName; Action = $Action; User = "$env:COMPUTERNAME\$ServiceUser"; RunLevel = 'Limited'; Force = $true }
        if ($Trigger) { $a.Trigger = $Trigger }
        if ($Settings) { $a.Settings = $Settings }
        if ($Description) { $a.Description = $Description }
        $a.Password = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($bstr)
        Register-ScheduledTask @a | Out-Null
    } finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($bstr)
        if ($a) { $a.Password = $null }
    }
}

# Windows creates a profile at an account's first logon. Start-Process -Credential does that in an
# interactive session; over SSH (no window station) it is refused, so a one-off scheduled task that
# runs as the account does it instead.
function New-DRServiceProfile([string]$Sid, [Security.SecureString]$Password) {
    $sys32 = Join-Path $env:WINDIR 'System32'
    try {
        $cred = New-Object Management.Automation.PSCredential("$env:COMPUTERNAME\$ServiceUser", $Password)
        Start-Process -FilePath (Join-Path $sys32 'cmd.exe') -ArgumentList '/c exit 0' -Credential $cred -LoadUserProfile -WorkingDirectory $sys32 -WindowStyle Hidden -Wait
    } catch {
        Write-DRInfo "no interactive logon possible here ($($_.Exception.Message.Trim())); using a one-off scheduled task"
        $name = 'Dauntless Revived profile setup'
        $action = New-ScheduledTaskAction -Execute (Join-Path $sys32 'cmd.exe') -Argument '/c exit 0' -WorkingDirectory $sys32
        Register-DRServiceTask -TaskName $name -Action $action -Password $Password
        try {
            Start-ScheduledTask -TaskName $name
            for ($i = 0; $i -lt 60; $i++) {
                Start-Sleep -Seconds 1
                if (Get-CimInstance Win32_UserProfile -Filter "SID='$Sid'" -ErrorAction SilentlyContinue) { break }
            }
        } finally { Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue }
    }
    return (Get-CimInstance Win32_UserProfile -Filter "SID='$Sid'" -ErrorAction SilentlyContinue)
}

# Turns password login off in an OpenSSH sshd_config: sets a global "PasswordAuthentication no" before
# any Match block (appending after one would scope it to that block only). Returns $true if it changed
# the file. Only ever called once key login is proven (the installer is running over this SSH session).
function Disable-DRSshPasswords([string]$ConfigPath) {
    $lines = @([IO.File]::ReadAllLines($ConfigPath))
    $matchAt = -1
    for ($i = 0; $i -lt $lines.Count; $i++) { if ($lines[$i] -match '^\s*Match\b') { $matchAt = $i; break } }
    $globalEnd = if ($matchAt -ge 0) { $matchAt } else { $lines.Count }
    $out = New-Object System.Collections.Generic.List[string]
    for ($i = 0; $i -lt $lines.Count; $i++) {
        # Drop any active global PasswordAuthentication line; keep ones inside a Match block untouched.
        if ($i -lt $globalEnd -and $lines[$i] -match '^\s*PasswordAuthentication\b') { continue }
        $out.Add($lines[$i])
    }
    $insertAt = if ($matchAt -ge 0) { for ($j = 0; $j -lt $out.Count; $j++) { if ($out[$j] -match '^\s*Match\b') { break } }; $j } else { $out.Count }
    $out.Insert($insertAt, 'PasswordAuthentication no')
    $text = ($out -join "`r`n") + "`r`n"
    if ([IO.File]::ReadAllText($ConfigPath) -ceq $text) { return $false }
    [IO.File]::WriteAllText($ConfigPath, $text, (New-Object System.Text.UTF8Encoding($false)))
    return $true
}

# Inbound firewall rules that currently let anyone reach a local TCP port.
function Get-DRRulesForPort([int]$Port, [string]$Protocol = 'TCP') {
    $pf = @(Get-NetFirewallPortFilter -Protocol $Protocol -ErrorAction SilentlyContinue | Where-Object {
        $lp = @($_.LocalPort)
        ($lp -contains "$Port") -or ($lp | Where-Object { $_ -match '^(\d+)-(\d+)$' -and [int]$Matches[1] -le $Port -and [int]$Matches[2] -ge $Port })
    })
    @($pf | Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object { $_.Direction -eq 'Inbound' -and $_.Action -eq 'Allow' -and $_.Enabled -eq 'True' })
}

$exitCode = 0
try {
    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Checking this machine'
    # ---------------------------------------------------------------------------------------------
    $P = Get-DRPaths $InstallRoot
    $Root = $P.Root
    $existingCfg = $null
    if (Test-Path -LiteralPath $P.ServerJson) { $existingCfg = Get-DRConfig $Root }
    if (-not $Mode) { $Mode = if ($existingCfg) { Get-DRMode $existingCfg } else { 'Public' } }
    $Public = ($Mode -eq 'Public')

    # Ports: explicit, else the existing install's, else the defaults (sandbox: the spare 620xx set).
    $portDefaults = if ($Sandbox) { $DRSandboxPorts } else { $DRDefaultPorts }
    $ports = [ordered]@{}
    foreach ($k in 'metagame', 'deploy', 'content', 'gateway', 'allowlist') {
        $explicit = @{ metagame = 'MetagamePort'; deploy = 'DeployPort'; content = 'ContentPort'; gateway = 'GatewayPort'; allowlist = 'AllowlistPort' }[$k]
        if ($PSBoundParameters.ContainsKey($explicit)) { $ports[$k] = [int](Get-Variable -Name $explicit -ValueOnly) }
        elseif ($existingCfg -and (Get-DRConfigValue $existingCfg 'Ports' $null) -and (Get-DRConfigValue $existingCfg.Ports $k $null)) { $ports[$k] = [int]$existingCfg.Ports.$k }
        else { $ports[$k] = [int]$portDefaults[$k] }
    }

    if ($Sandbox -and -not $PSBoundParameters.ContainsKey('InstallRoot')) { Stop-DR '-Sandbox needs an explicit -InstallRoot (a scratch folder).' }
    if ($Sandbox -and -not $GameDir) { Stop-DR '-Sandbox needs -GameDir (a stand-in game folder); it never extracts the real game.' }
    if ($Sandbox) {
        foreach ($k in $ports.Keys) {
            if ($ports[$k] -lt 62000 -or $ports[$k] -gt 62499) { Stop-DR "-Sandbox uses spare ports only (62000-62499); $k is $($ports[$k])." }
        }
        $AllowlistDryRun = $true
    }
    if ($ContentManifest -and -not $Sandbox) { Stop-DR '-ContentManifest is for sandbox tests only; a real server serves the verified manifest.' }
    if (@($GameZip, $GameZipUrl, $GameDir | Where-Object { $_ }).Count -gt 1) { Stop-DR 'Use one of -GameZip, -GameZipUrl or -GameDir.' }
    if ($GameZipUrl -and $GameZipUrl -notmatch '^https://[^\s]+$') { Stop-DR '-GameZipUrl must be an https:// URL.' }
    if (@($SourceZip, $SourceDir, $Ref | Where-Object { $_ }).Count -gt 1) { Stop-DR 'Use one of -SourceZip, -SourceDir or -Ref.' }
    if ($SourceCommit -and $SourceCommit -notmatch '^[0-9a-f]{7,40}(-dirty|-worktree)?$') { Stop-DR '-SourceCommit must be a git commit id.' }
    if (-not (Test-DRServerName $ServerName)) { Stop-DR '-ServerName must be 1-64 characters without line breaks, quotes or backslashes, and without spaces at either end.' }
    if ($Public -and ($TailscaleAuthKey -or $TailscaleShareUrl)) { Stop-DR 'The Tailscale options belong to -Mode Private.' }
    if ($TailscaleShareUrl -and -not (Test-DRShareUrl $TailscaleShareUrl)) { Stop-DR '-TailscaleShareUrl must start with https://login.tailscale.com/' }
    if ($AdvertiseHost -and -not (Test-DRHost $AdvertiseHost.ToLowerInvariant())) { Stop-DR '-AdvertiseHost must be an IPv4 address or a DNS name.' }
    $used = @('metagame', 'deploy', 'content'); if ($Public) { $used += 'gateway', 'allowlist' }
    if (@($used | ForEach-Object { $ports[$_] } | Sort-Object -Unique).Count -ne $used.Count) { Stop-DR "The ports must differ ($(($used | ForEach-Object { "$_=$($ports[$_])" }) -join ', '))." }
    if ($TailscaleAuthKey -and $TailscaleAuthKey -notmatch '^tskey-') { Write-DRWarn 'The Tailscale auth key does not start with tskey- (using it anyway).' }
    # "a,b" arrives as one string through powershell -File; accept both forms.
    $AdminIp = @($AdminIp | ForEach-Object { "$_" -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    foreach ($a in @($AdminIp)) { if ($a -and -not (Test-DRIPv4Cidr $a)) { Stop-DR "-AdminIp '$a' must be an IPv4 address or IPv4/prefix." } }
    if (-not $AdminIp -and $existingCfg) { $AdminIp = @(Get-DRConfigValue $existingCfg 'AdminIp' @()) | Where-Object { $_ } }
    # Text chat: -Chat, else what server.json has, else On. Private mode stays off.
    $ChatSetting = Resolve-DRChat $Chat $existingCfg
    if (-not $Public -and $ChatSetting -eq 'On') {
        if ($Chat -eq 'On') { Stop-DR 'Chat works in public mode only for now (private mode comes later, roadmap 3.10). Leave out -Chat On, or use -Mode Public.' }
        Write-DRInfo 'private mode has no remote chat yet, so it is off'
        $ChatSetting = 'Off'
    }

    Write-DRInfo "install root : $Root$(if ($Sandbox) { '   (SANDBOX: no system changes)' })"
    Write-DRInfo "mode         : $Mode$(if ($Public) { ' (friends connect through the TLS gateway)' } else { ' (friends connect over Tailscale)' })"

    $os = Get-CimInstance Win32_OperatingSystem
    $build = [int]$os.BuildNumber
    $instType = (Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction SilentlyContinue).InstallationType
    $ramGB = [math]::Round($os.TotalVisibleMemorySize / 1MB, 1)
    $isAdmin = Test-DRAdmin
    $problems = New-Object System.Collections.Generic.List[string]
    function Need([bool]$Ok, [string]$Text, [string]$Fix) {
        if ($Ok) { Write-DROk $Text } elseif ($Sandbox) { Write-DRWarn "$Text (sandbox: not required)" } else { Write-DRWarn "$Text - $Fix"; $problems.Add($Text) }
    }
    Need $isAdmin "running as administrator: $isAdmin" 'run from an elevated PowerShell (Run as administrator)'
    Need ([Environment]::Is64BitOperatingSystem -and [Environment]::Is64BitProcess) '64-bit Windows and 64-bit PowerShell' 'use the 64-bit Windows PowerShell'
    Need ($build -ge 17763) "Windows build $build ($($os.Caption))" 'needs Windows Server 2019 (build 17763) or newer'
    if (-not $Sandbox -and $build -ge 17763 -and $os.ProductType -eq 1) { Write-DRWarn 'this is a Windows client edition, not Windows Server; it works, but the kit is tested for Server 2019' }
    Need ($instType -ne 'Server Core') "installation type: $instType" 'Server Core cannot run the game servers; install Windows Server with the Desktop Experience'
    $sys32 = Join-Path $env:WINDIR 'System32'
    $desktopDlls = @('d3d11.dll', 'dxgi.dll', 'd3d9.dll', 'dsound.dll', 'opengl32.dll', 'UIAutomationCore.dll', 'hid.dll')
    $missingDesk = @($desktopDlls | Where-Object { -not (Test-Path -LiteralPath (Join-Path $sys32 $_)) })
    Need ($missingDesk.Count -eq 0) "Desktop Experience DLLs the game imports$(if ($missingDesk.Count) { " (missing: $($missingDesk -join ', '))" })" 'install the Desktop Experience'
    Need ($PSVersionTable.PSVersion.Major -ge 5) "PowerShell $($PSVersionTable.PSVersion)" 'needs Windows PowerShell 5.1'
    # A VPS sold as "8 GB" shows a little less than 8 GB to Windows.
    Need ($ramGB -ge 7.5) "RAM: $ramGB GB" 'needs 8 GB or more (Ramsgate ~1.1 GB, each hunt ~1 GB, the rest of the stack ~0.5 GB)'

    # Disk: the zip (10.5 GB), the extracted game (10.9 GB), the server and backups fit in a 40 GB budget.
    $drive = [IO.Path]::GetPathRoot($Root)
    $free = [double](Get-CimInstance Win32_LogicalDisk -Filter ("DeviceID='{0}'" -f $drive.TrimEnd('\'))).FreeSpace
    $gameInstalled = Test-Path -LiteralPath (Join-Path (Join-Path $P.Game 'Dauntless') $DRPinned.ExeRelative)
    $budget = $free
    if ($GameZip -and (Test-Path -LiteralPath $GameZip) -and ([IO.Path]::GetPathRoot([IO.Path]::GetFullPath($GameZip)) -eq $drive)) { $budget += (Get-Item -LiteralPath $GameZip).Length }
    if ($gameInstalled) { $budget += 11GB }
    $needGB = if ($Sandbox) { 2 } else { 40 }
    Need ($budget / 1GB -ge $needGB) ("disk on {0} {1:N1} GB free ({2:N1} GB counting the game zip and game already there; need {3} GB)" -f $drive, ($free / 1GB), ($budget / 1GB), $needGB) 'free up space or choose another -InstallRoot'
    if (($GameZip -or $GameZipUrl -or $SourceZip -or -not $Sandbox) -and -not (Test-Path -LiteralPath (Get-DRTar))) { $problems.Add('tar.exe'); Write-DRWarn 'tar.exe not found (Windows 10 1803 / Server 2019 and newer have it)' }

    # Inputs
    if ($GameZip -and -not (Test-Path -LiteralPath $GameZip)) { Stop-DR "Game zip not found: $GameZip" }
    if ($GameDir -and -not (Test-Path -LiteralPath (Join-Path $GameDir 'Archon'))) { Stop-DR "-GameDir must be the folder that contains Archon\ ($GameDir)" }
    if (-not $GameZip -and -not $GameZipUrl -and -not $GameDir -and -not $gameInstalled) {
        if ($existingCfg -and (Get-DRConfigValue $existingCfg 'GameDir' '') -and (Test-Path -LiteralPath (Join-Path (Get-DRConfigValue $existingCfg 'GameDir' '') 'Archon'))) { $GameDir = Get-DRConfigValue $existingCfg 'GameDir' '' }
        else { Stop-DR 'The game is not installed yet: pass -GameZip <the verified 1.4.4 zip> or -GameZipUrl <https URL>.' }
    }
    if ($SourceZip -and -not (Test-Path -LiteralPath $SourceZip)) { Stop-DR "Source zip not found: $SourceZip" }
    if ($ContentManifest -and -not (Test-Path -LiteralPath $ContentManifest)) { Stop-DR "Manifest not found: $ContentManifest" }
    $restore = $null
    if ($RestoreFrom) {
        $restore = [IO.Path]::GetFullPath($RestoreFrom)
        foreach ($f in 'undaunted.db', 'secrets\metagame.env', 'secrets\deployserver.env') {
            if (-not (Test-Path -LiteralPath (Join-Path $restore $f))) { Stop-DR "-RestoreFrom: $f is missing in $restore" }
        }
        Write-DROk "restoring from $restore"
    }
    if (-not $restore -and -not (Test-Path -LiteralPath $P.OwnerKey)) {
        if (-not $OwnerName -and -not $IsWhatIf -and [Environment]::UserInteractive -and -not [Console]::IsInputRedirected) {
            $OwnerName = (Read-Host 'Username for your own (admin) account, 3-16 letters, digits or _').Trim()
        }
        if ($OwnerName -and -not (Test-DRUsername $OwnerName)) { Stop-DR 'Usernames are 3-16 characters: letters, digits or _.' }
        if (-not $OwnerName -and -not $IsWhatIf) { Stop-DR 'An owner username is needed (-OwnerName), or a backup to restore (-RestoreFrom).' }
    }

    # Public address
    $MyIp = $null; $PubHost = $null
    if ($Public) {
        $PubHost = if ($PublicHost) { $PublicHost.Trim().ToLowerInvariant() } else { [string](Get-DRConfigValue $existingCfg 'PublicHost' '') }
        if (-not $PubHost) {
            if ($Sandbox) { $PubHost = '127.0.0.1' }
            else {
                $nic = @(Get-DRLocalPublicIPv4)
                if ($nic.Count -eq 1) { $PubHost = $nic[0]; Write-DROk "public address from the network adapter: $PubHost" }
                elseif ($nic.Count -gt 1) { Stop-DR "This machine has several public IPv4 addresses ($($nic -join ', ')). Pass -PublicHost." }
                else { Stop-DR 'No public IPv4 address on the network adapters (the VPS is probably behind NAT). Pass -PublicHost <the public IP or DNS name>.' }
            }
        }
        if (-not (Test-DRHost $PubHost)) { Stop-DR "-PublicHost '$PubHost' must be an IPv4 address or a DNS name." }
        $MyIp = Resolve-DRIPv4 $PubHost
        if (-not $MyIp) { Stop-DR "-PublicHost '$PubHost' does not resolve to an IPv4 address." }
        if (-not $Sandbox -and -not (Test-DRPublicIPv4 $MyIp)) { Stop-DR "-PublicHost '$PubHost' is $MyIp, which is not a public address." }
        Write-DROk "friends connect to ${PubHost}:$($ports.gateway) (game servers advertise $MyIp)"
        if (-not $Sandbox -and -not ((Get-DRLocalPublicIPv4) -contains $MyIp)) {
            Write-DRInfo "$MyIp is not on this machine's adapters (1:1 NAT): make sure the provider forwards TCP $($ports.gateway) and UDP $UdpBegin-$UdpEnd to this server"
        }
    }

    # A running stack must not have its code or game swapped underneath it.
    $running = @($DRComponentOrder | Where-Object { @(Get-DRComponentProcesses $P $_).Count })
    if ($running.Count) {
        if ($IsWhatIf) { Write-DRWarn "the stack is running ($($running -join ', ')); a real run would ask you to stop it first" }
        else { Stop-DR "The stack is running ($($running -join ', ')). Stop it first ($($P.Bin)\Stack.ps1 stop), or use Update-DauntlessServer.ps1 for code updates." }
    }
    $checkPorts = @('metagame', 'content'); if (-not $Sandbox) { $checkPorts += 'deploy' }; if ($Public) { $checkPorts += 'gateway', 'allowlist' }
    foreach ($k in $checkPorts) {
        $o = Get-DRPortOwner $ports[$k]
        if ($o) { $problems.Add("port $($ports[$k])"); Write-DRWarn "TCP $($ports[$k]) ($k) is taken by $($o.Name) (pid $($o.Pid)); choose another port or stop that program" }
    }
    if (-not $Sandbox) {
        $udp = @(Get-NetUDPEndpoint -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -ge $UdpBegin -and $_.LocalPort -le $UdpEnd })
        if ($udp.Count) { Write-DRWarn "UDP $UdpBegin-$UdpEnd already has listeners (ports $(@($udp.LocalPort | Sort-Object -Unique) -join ', ')); game servers need them" }
    }
    if ($problems.Count) {
        if ($IsWhatIf) { Write-DRWarn "a real run would stop here: $($problems -join '; ')" }
        else { Stop-DR "Fix these first: $($problems -join '; ')" }
    }
    if ($existingCfg) { Write-DRInfo "an earlier install exists here ($(Get-DRMode $existingCfg) mode): completing / repairing it" }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Folders'
    # ---------------------------------------------------------------------------------------------
    foreach ($d in @($P.Root, $P.Bin, $P.Data, $P.Config, $P.Keys, $P.Tls, $P.Logs, $P.Run, $P.Branding, $P.Backups, $P.Downloads, (Join-Path $P.Logs 'install'), (Join-Path $P.Data 'allowlist'))) {
        if (-not (Test-Path -LiteralPath $d)) {
            if (Test-Do $d 'Create folder') { New-Item -ItemType Directory -Force -Path $d | Out-Null }
        }
    }
    # The kit's scripts go to <root>\bin, from the copy of the kit that is running now.
    if ([IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\') -ne $P.Bin -and (Test-Do $P.Bin 'Copy the kit scripts')) {
        foreach ($f in Get-ChildItem -LiteralPath $PSScriptRoot -File | Where-Object { $_.Extension -in '.ps1', '.vbs', '.md' }) {
            Copy-Item -LiteralPath $f.FullName -Destination $P.Bin -Force
        }
        New-Item -ItemType Directory -Force -Path (Join-Path $P.Bin 'lib') | Out-Null
        Get-ChildItem -LiteralPath (Join-Path $PSScriptRoot 'lib') -File | Where-Object { $_.Extension -in '.js', '.cjs' } | ForEach-Object { Copy-Item -LiteralPath $_.FullName -Destination (Join-Path $P.Bin 'lib') -Force }
    }
    Write-DROk "folders under $Root"

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Prerequisites'
    # ---------------------------------------------------------------------------------------------
    Enable-DRTls12
    $NodeExe = Join-Path $env:ProgramFiles 'nodejs\node.exe'
    if ($Sandbox) {
        $cmd = Get-Command node.exe -ErrorAction SilentlyContinue
        if (-not $cmd) { Stop-DR 'Node.js is not on PATH.' }
        $NodeExe = $cmd.Source
        Write-DRSkip "sandbox: using the Node.js already installed ($NodeExe, $((& $NodeExe --version)))"
        Write-DRSkip 'sandbox: Visual C++ runtime, DirectX runtime and Tailscale not installed'
    } else {
        # Node.js
        $nodeOk = $false
        if (Test-Path -LiteralPath $NodeExe) {
            $v = (& $NodeExe --version).Trim().TrimStart('v')
            if ($v -eq $DRPinned.NodeVersion) { Write-DROk "Node.js $v"; $nodeOk = $true }
            elseif ($v.Split('.')[0] -eq $DRPinned.NodeVersion.Split('.')[0]) { Write-DROk "Node.js $v (same major version as the pinned $($DRPinned.NodeVersion); keeping it)"; $nodeOk = $true }
            else { Write-DRWarn "Node.js $v found; installing the pinned $($DRPinned.NodeVersion)" }
        }
        if (-not $nodeOk) {
            $ver = $DRPinned.NodeVersion
            $msiName = "node-v$ver-x64.msi"
            $msi = Join-Path $P.Downloads $msiName
            if (Test-Do "Node.js $ver ($msiName from nodejs.org)" 'Download, verify and install') {
                Save-DRDownload "https://nodejs.org/dist/v$ver/$msiName" $msi
                $sums = Get-DRDownloadText "https://nodejs.org/dist/v$ver/SHASUMS256.txt"
                $line = ($sums -split "`n" | Where-Object { $_ -match "\s$([regex]::Escape($msiName))\s*$" } | Select-Object -First 1)
                $published = if ($line) { ($line.Trim() -split '\s+')[0].ToUpperInvariant() } else { '' }
                $actual = Get-DRSha256 $msi
                if ($published -ne $DRPinned.NodeMsiSha256) { Stop-DR "SHASUMS256.txt for Node.js $ver does not list the pinned hash for $msiName. Not installing." }
                if ($actual -ne $DRPinned.NodeMsiSha256) { Stop-DR "$msiName does not match its pinned SHA-256. Not installing." }
                Write-DROk "$msiName matches the pinned SHA-256 and SHASUMS256.txt"
                $code = Invoke-DRNative -FilePath 'msiexec.exe' -Arguments @('/i', $msi, '/qn', '/norestart', '/l*v', (Join-Path $P.Logs 'install\node-msi.log')) -LogBase (Join-Path $P.Logs 'install\node-msiexec')
                if ($code -ne 0 -and $code -ne 3010) { Stop-DR "Node.js installer failed (exit code $code, log $($P.Logs)\install\node-msi.log)" }
                Write-DROk "Node.js $ver installed"
            }
        }
        # A fresh MSI install reaches PATH only in new logon sessions, but npm's install scripts
        # (cmd.exe -> node, e.g. better-sqlite3's prebuild-install) run in this one.
        $nodeDir = Split-Path -Parent $NodeExe
        if (($env:Path -split ';') -notcontains $nodeDir) { $env:Path = "$nodeDir;$env:Path"; Write-DROk "Node.js added to this session's PATH ($nodeDir)" }
        # Visual C++ 2015-2022 x64 (UndauntedInternalServer.dll imports MSVCP140 and VCRUNTIME140_1)
        $vcMissing = @('MSVCP140.dll', 'VCRUNTIME140.dll', 'VCRUNTIME140_1.dll') | Where-Object { -not (Test-Path -LiteralPath (Join-Path $sys32 $_)) }
        if (-not $vcMissing) { Write-DROk 'Visual C++ 2015-2022 x64 runtime' }
        elseif (Test-Do 'Visual C++ 2015-2022 x64 runtime (aka.ms/vs/17/release/vc_redist.x64.exe)' 'Download, verify signature and install') {
            $vc = Join-Path $P.Downloads 'vc_redist.x64.exe'
            Save-DRDownload $DRPinned.VcRedistUrl $vc
            $sig = Test-DRSignature $vc 'Microsoft Corporation'
            if (-not $sig.Ok) { Stop-DR "vc_redist.x64.exe is not validly signed by Microsoft ($($sig.Status)). Not installing." }
            $code = Invoke-DRNative -FilePath $vc -Arguments @('/install', '/quiet', '/norestart') -LogBase (Join-Path $P.Logs 'install\vcredist')
            if ($code -notin 0, 1638, 3010) { Stop-DR "Visual C++ runtime installer failed (exit code $code)" }
            Write-DROk 'Visual C++ runtime installed'
        }
        # DirectX End-User Runtime (June 2010): the game exe imports XINPUT1_3, X3DAudio1_7 and XAPOFX1_5.
        $dxMissing = @('XINPUT1_3.dll', 'X3DAudio1_7.dll', 'XAPOFX1_5.dll', 'D3DCompiler_43.dll') | Where-Object { -not (Test-Path -LiteralPath (Join-Path $sys32 $_)) }
        if (-not $dxMissing) { Write-DROk 'DirectX June 2010 runtime (XINPUT1_3, X3DAudio1_7, XAPOFX1_5)' }
        elseif (Test-Do 'DirectX End-User Runtime June 2010 (download.microsoft.com, ~96 MB)' 'Download, verify signature and install') {
            $dx = Join-Path $P.Downloads 'directx_Jun2010_redist.exe'
            Save-DRDownload $DRPinned.DirectXUrl $dx
            $sig = Test-DRSignature $dx 'Microsoft Corporation'
            if (-not $sig.Ok) { Stop-DR "directx_Jun2010_redist.exe is not validly signed by Microsoft ($($sig.Status)). Not installing." }
            $h = Get-DRSha256 $dx
            if ($DRPinned.DirectXKnownSha256 -contains $h) { Write-DROk 'DirectX package matches a known SHA-256' }
            else { Write-DRWarn "DirectX package SHA-256 $h is not one we have seen; its Microsoft signature is valid, continuing" }
            $dxDir = Join-Path $P.Downloads 'dxredist'
            if (Test-Path -LiteralPath $dxDir) { Remove-Item -LiteralPath $dxDir -Recurse -Force }
            New-Item -ItemType Directory -Force -Path $dxDir | Out-Null
            $code = Invoke-DRNative -FilePath $dx -Arguments @('/Q', "/T:$dxDir") -LogBase (Join-Path $P.Logs 'install\dx-extract')
            $setup = Join-Path $dxDir 'DXSETUP.exe'
            if (-not (Test-Path -LiteralPath $setup)) { Stop-DR "Extracting the DirectX package failed (exit code $code)" }
            if (-not (Test-DRSignature $setup 'Microsoft Corporation').Ok) { Stop-DR 'DXSETUP.exe is not validly signed by Microsoft. Not installing.' }
            $code = Invoke-DRNative -FilePath $setup -Arguments @('/silent') -WorkingDirectory $dxDir -LogBase (Join-Path $P.Logs 'install\dxsetup')
            $still = @('XINPUT1_3.dll', 'X3DAudio1_7.dll', 'XAPOFX1_5.dll') | Where-Object { -not (Test-Path -LiteralPath (Join-Path $sys32 $_)) }
            if ($code -ne 0 -or $still) { Stop-DR "DirectX runtime setup failed (exit code $code; still missing: $($still -join ', '))" }
            Write-DROk 'DirectX June 2010 runtime installed'
        }
        # Media Foundation is only delay-loaded by the game (video playback); game servers do not need it.
        if (-not $Public) {
            $tsExe = Get-DRTailscaleExe
            if ($tsExe) { Write-DROk "Tailscale installed ($tsExe)" }
            elseif (Test-Do "Tailscale $($DRPinned.TailscaleVersion) (pkgs.tailscale.com)" 'Download, verify and install') {
                $tsName = "tailscale-setup-$($DRPinned.TailscaleVersion)-amd64.msi"
                $tsMsi = Join-Path $P.Downloads $tsName
                Save-DRDownload "https://pkgs.tailscale.com/stable/$tsName" $tsMsi
                $published = ((Get-DRDownloadText "https://pkgs.tailscale.com/stable/$tsName.sha256") -split '\s+')[0].Trim().ToUpperInvariant()
                $actual = Get-DRSha256 $tsMsi
                if ($published -ne $DRPinned.TailscaleMsiSha256 -or $actual -ne $DRPinned.TailscaleMsiSha256) { Stop-DR "$tsName does not match its pinned SHA-256. Not installing." }
                $sig = Test-DRSignature $tsMsi 'Tailscale Inc.'
                if (-not $sig.Ok) { Write-DRWarn "Tailscale MSI signature: $($sig.Status) $($sig.Subject) (the pinned SHA-256 matched)" }
                $code = Invoke-DRNative -FilePath 'msiexec.exe' -Arguments @('/i', $tsMsi, '/qn', '/norestart', 'TS_NOLAUNCH=yes', '/l*v', (Join-Path $P.Logs 'install\tailscale-msi.log')) -LogBase (Join-Path $P.Logs 'install\tailscale-msiexec')
                if ($code -ne 0 -and $code -ne 3010) { Stop-DR "Tailscale installer failed (exit code $code)" }
                $tsExe = Get-DRTailscaleExe
                Write-DROk 'Tailscale installed'
            }
        } else { Write-DRSkip 'public mode: no Tailscale' }
    }
    if (-not (Test-Path -LiteralPath $NodeExe) -and $IsWhatIf) { Write-DRInfo "(what-if: Node.js would be at $NodeExe)" }

    # Where the metagame and content server listen.
    $BindIp = '127.0.0.1'
    $MagicDns = ''
    if (-not $Public) {
        if ($Sandbox) { Write-DRSkip 'sandbox: listening on 127.0.0.1 instead of a Tailscale address' }
        else {
            $BindIp = Get-DRTailscaleIPv4
            if (-not $BindIp -and $tsExe) {
                if ($TailscaleAuthKey) {
                    if (Test-Do "Tailscale login as '$TailscaleHostname'" 'tailscale up with the auth key') {
                        $keyFile = Join-Path $P.Keys 'tailscale-authkey.tmp'
                        Write-DRText -Path $keyFile -Text $TailscaleAuthKey
                        Set-DRAcl $keyFile @{ $SidAdministrators = 'FullControl'; $SidSystem = 'FullControl' }
                        try {
                            $code = Invoke-DRNative -FilePath $tsExe -Arguments @('up', '--unattended', "--hostname=$TailscaleHostname", "--auth-key=file:$keyFile") -LogBase (Join-Path $P.Logs 'install\tailscale-up')
                        } finally { Remove-Item -LiteralPath $keyFile -Force -ErrorAction SilentlyContinue }
                        if ($code -ne 0) { Stop-DR "tailscale up failed (exit code $code; see $($P.Logs)\install\tailscale-up.err.log)" }
                    }
                } elseif (-not $IsWhatIf) {
                    Write-Host ''
                    Write-Host '   Sign this machine in to Tailscale. In another elevated PowerShell, run:' -ForegroundColor Yellow
                    Write-Host "     & '$tsExe' up --unattended --hostname=$TailscaleHostname"
                    Write-Host '   and open the link it prints. Waiting up to 20 minutes ...' -ForegroundColor Yellow
                }
                if (-not $IsWhatIf) {
                    for ($i = 0; $i -lt 240 -and -not $BindIp; $i++) { Start-Sleep -Seconds 5; $BindIp = Get-DRTailscaleIPv4 }
                    if (-not $BindIp) { Stop-DR 'This machine did not join the tailnet. Run the command above, then run this script again.' }
                }
            }
            if ($BindIp) {
                Write-DROk "Tailscale address $BindIp"
                $self = Get-DRTailscaleSelf
                if ($self -and $self.DNSName) { $MagicDns = "$($self.DNSName)".TrimEnd('.').ToLowerInvariant(); Write-DROk "MagicDNS name $MagicDns" }
                $prof = Get-NetConnectionProfile -ErrorAction SilentlyContinue | Where-Object { $_.InterfaceAlias -eq (Get-DRTailscaleAdapter).Name }
                if ($prof -and $prof.NetworkCategory -eq 'Private' -and (Test-Do 'Tailscale network adapter' 'Set the network category to Public')) {
                    Set-NetConnectionProfile -InterfaceIndex $prof.InterfaceIndex -NetworkCategory Public
                    Write-DROk 'Tailscale adapter set to the Public network category'
                }
            } else {
                $BindIp = '100.64.0.1'
                Write-DRInfo "(what-if: using the placeholder address $BindIp below)"
            }
        }
        $MyIp = $BindIp
    }
    $GatewayBind = if ($Sandbox) { '127.0.0.1' } else { '0.0.0.0' }
    $Advertise = if ($Public) { $PubHost } elseif ($AdvertiseHost) { $AdvertiseHost.ToLowerInvariant() } elseif ($existingCfg -and (Get-DRConfigValue $existingCfg 'AdvertiseHost' '') -and (Get-DRMode $existingCfg) -eq 'Private') { Get-DRConfigValue $existingCfg 'AdvertiseHost' '' } else { $BindIp }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Server code'
    # ---------------------------------------------------------------------------------------------
    $checkout = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
    $source = $null; $sourceKind = ''; $commit = 'unknown'; $refUsed = ''
    if ($SourceZip) {
        $sourceKind = 'zip'
        $commit = if ($SourceCommit) { $SourceCommit } else { 'zip-' + (Get-DRSha256 $SourceZip).Substring(0, 12).ToLowerInvariant() }
        Write-DROk "source: uploaded zip $SourceZip (commit $commit)"
    } elseif ($SourceDir) { $source = [IO.Path]::GetFullPath($SourceDir); $sourceKind = 'folder' }
    elseif (-not $Ref -and (Test-Path -LiteralPath (Join-Path $checkout 'UndauntedMetagame\package.json'))) { $source = $checkout; $sourceKind = 'checkout' }
    if ($source) {
        if (-not (Test-Path -LiteralPath (Join-Path $source 'UndauntedMetagame\package.json'))) { Stop-DR "$source is not a Dauntless Revived source folder." }
        $git = Get-Command git.exe -ErrorAction SilentlyContinue
        if ($git -and (Test-Path -LiteralPath (Join-Path $source '.git'))) {
            $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
            try {
                $commit = (& git.exe -C $source rev-parse HEAD 2>$null | Out-String).Trim()
                $refUsed = (& git.exe -C $source rev-parse --abbrev-ref HEAD 2>$null | Out-String).Trim()
                if ((& git.exe -C $source status --porcelain 2>$null | Out-String).Trim()) { $commit = "$commit-dirty" }
            } finally { $ErrorActionPreference = $old }
            if (-not $commit) { $commit = 'unknown' }
        }
        Write-DROk "source: $sourceKind $source (commit $commit)"
    } elseif (-not $SourceZip) {
        $refUsed = if ($Ref) { $Ref } else { $DRRepo.PinnedRef }
        if ($refUsed -notmatch '^[A-Za-z0-9._/-]{1,100}$') { Stop-DR "-Ref '$refUsed' does not look like a branch, tag or commit." }
        $sourceKind = 'github'
        try {
            Enable-DRTls12
            $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 60 -Uri ("https://api.github.com/repos/{0}/{1}/commits/{2}" -f $DRRepo.Owner, $DRRepo.Name, $refUsed) -Headers @{ Accept = 'application/vnd.github.sha' }
            $commit = "$($r.Content)".Trim()
        } catch { Stop-DR "GitHub does not know the ref '$refUsed' in $($DRRepo.Url) ($($_.Exception.Message)). Pass -Ref <tag, branch or commit>." }
        if ($commit -notmatch '^[0-9a-f]{40}$') { Stop-DR "Could not resolve '$refUsed' to a commit." }
        Write-DROk "source: $($DRRepo.Url) at $refUsed = $commit"
    }

    $versionFile = Join-Path $P.App 'VERSION.json'
    $built = (Test-Path -LiteralPath $versionFile) -and (Test-Path -LiteralPath (Join-Path $P.App 'UndauntedMetagame\dist\server.js'))
    $sameCode = $false
    if ($built) {
        $prev = Get-Content -LiteralPath $versionFile -Raw | ConvertFrom-Json
        $sameCode = ($prev.commit -eq $commit -and $commit -ne 'unknown' -and $commit -notmatch '-(dirty|worktree)$')
        if ($sameCode -and $Public -and -not (Test-Path -LiteralPath (Get-DRComponentScript $P 'gateway'))) { $sameCode = $false }
    }
    if ($sameCode) {
        Write-DROk "code $commit is already installed and built"
    } elseif (Test-Do "$($P.App) (from $sourceKind)" 'Copy the server code and build it with npm ci + npm run build') {
        $copyFrom = $source
        if ($sourceKind -eq 'github') {
            $zip = Join-Path $P.Downloads "source-$commit.zip"
            if (-not (Test-Path -LiteralPath $zip)) { Save-DRDownload ("https://codeload.github.com/{0}/{1}/zip/{2}" -f $DRRepo.Owner, $DRRepo.Name, $commit) $zip }
            $copyFrom = Expand-DRSourceZip $P $zip $commit.Substring(0, 12)
        } elseif ($sourceKind -eq 'zip') {
            $copyFrom = Expand-DRSourceZip $P ([IO.Path]::GetFullPath($SourceZip)) 'upload'
        }
        $ver = [ordered]@{ commit = $commit; ref = $refUsed; source = $sourceKind; sourceUrl = $DRRepo.Url; installedAt = (Get-Date).ToString('o') }
        $builtPkgs = Build-DRApp -Paths $P -From $copyFrom -NodeExe $NodeExe -Version $ver
        if ($Public -and $builtPkgs -notcontains 'UndauntedGateway') { Stop-DR 'This code version has no UndauntedGateway; public mode needs it. Use newer code, or -Mode Private.' }
        Switch-DRApp $P
        Write-DROk "server code $commit installed in $($P.App)"
    }
    $hasPkg = { param($d) (Test-Path -LiteralPath (Join-Path (Join-Path $P.App $d) 'package.json')) -or ($IsWhatIf -and -not (Test-Path -LiteralPath $P.App)) }
    $contentAvailable = & $hasPkg 'UndauntedContent'
    if ($Public -and -not (& $hasPkg 'UndauntedGateway') -and -not $IsWhatIf) { Stop-DR 'Public mode needs UndauntedGateway, which the installed code does not have.' }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Game files'
    # ---------------------------------------------------------------------------------------------
    if ($GameZipUrl -and -not $gameInstalled) {
        $dl = Join-Path $P.Downloads 'BaseGame144.zip'
        if (Test-Do "$dl (from $(([Uri]$GameZipUrl).Host))" 'Download the game zip (10.5 GB, resumable) and check its SHA-256') {
            Write-DRInfo 'downloading the game zip (resumes after interruptions; a re-run continues where it stopped) ...'
            try { Save-DRLargeDownload -Url $GameZipUrl -OutFile $dl -Sha256 $DRPinned.GameZipSha256 }
            catch { Stop-DR "Downloading the game zip failed: $($_.Exception.Message)" }
            Write-DROk 'downloaded; the zip is the verified 1.4.4 build'
            $GameZip = $dl
        }
    }
    $GameRoot = if ($GameDir) { [IO.Path]::GetFullPath($GameDir).TrimEnd('\') } else { Join-Path $P.Game 'Dauntless' }
    $exe = Join-Path $GameRoot $DRPinned.ExeRelative
    $exeOk = (Test-Path -LiteralPath $exe) -and ((Get-DRSha256 $exe) -eq $DRPinned.ExeSha256)
    if (-not $exeOk -and -not $GameDir -and $GameZip -and (Test-Path -LiteralPath $GameZip)) {
        Write-DRInfo 'checking the zip against the verified 1.4.4 build (reads ~10.5 GB, a few minutes) ...'
        $zipHash = Get-DRSha256 $GameZip
        if ($zipHash -ne $DRPinned.GameZipSha256) { Stop-DR "The zip is not the verified 1.4.4 build (SHA-256 $zipHash). Not extracting." }
        Write-DROk 'the zip is the verified 1.4.4 build'
        if (Test-Do $P.Game 'Extract the game with tar.exe (~10.9 GB)') {
            $partial = "$($P.Game).partial"
            if (Test-Path -LiteralPath $partial) { Remove-Item -LiteralPath $partial -Recurse -Force }
            New-Item -ItemType Directory -Force -Path $partial | Out-Null
            Write-DRInfo 'extracting (10-20 minutes on a VPS disk) ...'
            $code = Invoke-DRNative -FilePath (Get-DRTar) -Arguments @('-xf', $GameZip, '-C', $partial) -LogBase (Join-Path $P.Logs 'install\game-extract') -LowPriority
            if ($code -ne 0 -or -not (Test-Path -LiteralPath (Join-Path $partial 'Dauntless\Archon'))) { Show-DRLogTail (Join-Path $P.Logs 'install\game-extract'); Stop-DR "Extracting the game failed (tar exit code $code)" }
            if (Test-Path -LiteralPath $P.Game) { Remove-Item -LiteralPath $P.Game -Recurse -Force }
            Rename-Item -LiteralPath $partial -NewName (Split-Path $P.Game -Leaf)
            $exeOk = (Get-DRSha256 $exe) -eq $DRPinned.ExeSha256
            Write-DROk "extracted to $GameRoot"
        }
    }
    $manifest = if ($ContentManifest) { [IO.Path]::GetFullPath($ContentManifest) } else { Join-Path $P.App 'UndauntedContent\data\dauntless-1.4.4.json' }
    if ($Sandbox) {
        if ($exeOk) { Write-DROk 'the game exe is the verified 1.4.4 build' }
        else { Write-DRWarn "sandbox: $GameRoot is a stand-in; the exe hash is not the 1.4.4 build (not enforced in a sandbox)" }
        if ($ContentManifest -and (Test-Path -LiteralPath (Join-Path $P.Bin 'lib\verify-game.js'))) {
            $code = Invoke-DRNative -FilePath $NodeExe -Arguments @((Join-Path $P.Bin 'lib\verify-game.js'), $manifest, $GameRoot) -LogBase (Join-Path $P.Logs 'install\verify-game')
            $summary = Get-Content -LiteralPath (Join-Path $P.Logs 'install\verify-game.out.log') | Select-Object -Last 1
            if ($code -ne 0) { Stop-DR "The stand-in game folder does not match its manifest: $summary" }
            Write-DROk "stand-in game folder: $summary"
        }
    } elseif (-not $IsWhatIf -or $exeOk) {
        if (-not $exeOk) { Stop-DR "$exe is missing or is not the verified 1.4.4 build." }
        Write-DROk 'the game exe is the verified 1.4.4 build (D3D41E61...CFF4)'
        $versionTxt = Join-Path $GameRoot 'Version.txt'
        if ((Test-Path -LiteralPath $versionTxt) -and ((Get-Content -LiteralPath $versionTxt -Raw).Trim() -eq $DRPinned.Build)) { Write-DROk "Version.txt: $($DRPinned.Build)" }
        else { Write-DRWarn 'Version.txt is missing or names another build' }
        if (Test-Path -LiteralPath $manifest) {
            Write-DRInfo 'checking every game file against the content manifest (SHA-256 of ~10.9 GB) ...'
            $code = Invoke-DRNative -FilePath $NodeExe -Arguments @((Join-Path $P.Bin 'lib\verify-game.js'), $manifest, $GameRoot) -LogBase (Join-Path $P.Logs 'install\verify-game') -LowPriority
            $summary = Get-Content -LiteralPath (Join-Path $P.Logs 'install\verify-game.out.log') | Select-Object -Last 1
            if ($code -ne 0) { Get-Content -LiteralPath (Join-Path $P.Logs 'install\verify-game.out.log') | Select-Object -First 20 | ForEach-Object { Write-DRInfo $_ }; Stop-DR "The game files do not match the manifest: $summary" }
            Write-DROk $summary
        } else { Write-DRWarn 'no content manifest in this code version; only the exe was checked' }
    } else {
        Write-DRInfo "(what-if: the game would be extracted to $GameRoot and checked)"
    }
    # The two server DLLs, checked before and after copying (same pins as the friend kit).
    $win64 = Split-Path $exe -Parent
    $assets = Join-Path $P.App 'UndauntedLauncher\assets'
    foreach ($dll in $DRPinned.Dlls.Keys) {
        $target = Join-Path $win64 $dll
        if ((Test-Path -LiteralPath $target) -and ((Get-DRSha256 $target) -eq $DRPinned.Dlls[$dll])) { Write-DROk "$dll in place"; continue }
        $src = Join-Path $assets $dll
        if (-not (Test-Path -LiteralPath $src)) {
            if ($IsWhatIf) { Write-DRInfo "(what-if: $dll would be copied from the built code)"; continue }
            Stop-DR "$dll is missing from $assets"
        }
        if ((Get-DRSha256 $src) -ne $DRPinned.Dlls[$dll]) { Stop-DR "$src does not match its pinned SHA-256. Not installing it." }
        if (Test-Do $target 'Install the pinned DLL') {
            if (-not (Test-Path -LiteralPath $win64)) { Stop-DR "$win64 does not exist" }
            Copy-Item -LiteralPath $src -Destination $target -Force
            Unblock-File -LiteralPath $target
            if ((Get-DRSha256 $target) -ne $DRPinned.Dlls[$dll]) { Stop-DR "$dll changed while copying. Is antivirus interfering?" }
            Write-DROk "$dll installed and checked"
        }
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Service account'
    # ---------------------------------------------------------------------------------------------
    $svcSid = $null; $svcProfile = $null
    if ($Sandbox) {
        $svcSid = Get-DRCurrentSid
        Write-DRSkip "sandbox: no service account; everything runs as $([Security.Principal.WindowsIdentity]::GetCurrent().Name)"
    } else {
        $pw = $null
        try {
            $u = Get-LocalUser -Name $ServiceUser -ErrorAction SilentlyContinue
            if ($u) { Write-DROk "local account '$ServiceUser' exists" }
            elseif (Test-Do "local account '$ServiceUser'" 'Create (random password, never shown; Task Scheduler keeps it encrypted for its tasks)') {
                $pw = New-DRPassword
                New-LocalUser -Name $ServiceUser -Password $pw -PasswordNeverExpires -UserMayNotChangePassword -AccountNeverExpires `
                    -FullName 'Dauntless Revived server' -Description 'Runs the Dauntless Revived server' | Out-Null  # Windows allows 48 characters at most
                Write-DROk "local account '$ServiceUser' created"
            }
            $svcSid = Get-DRLocalUserSid $ServiceUser
            if ($svcSid) {
                $inUsers = Get-LocalGroupMember -SID $SidUsers -ErrorAction SilentlyContinue | Where-Object { $_.SID.Value -eq $svcSid }
                if (-not $inUsers -and (Test-Do "'$ServiceUser'" 'Add to the local Users group')) { Add-LocalGroupMember -SID $SidUsers -Member $ServiceUser }
                $inAdmins = Get-LocalGroupMember -SID $SidAdministrators -ErrorAction SilentlyContinue | Where-Object { $_.SID.Value -eq $svcSid }
                if ($inAdmins) { Stop-DR "'$ServiceUser' is in the Administrators group. It must be a plain user; remove it from Administrators." }
                if (Test-Do "'$ServiceUser'" 'Grant "Log on as a batch job" (needed by its scheduled tasks)') {
                    Initialize-DRLsa
                    [DRKit.Lsa]::AddRight($svcSid, 'SeBatchLogonRight')
                    Write-DROk 'log on as a batch job: granted'
                }
                # The profile holds the game's user config (Game.ini, Engine.ini).
                $prof = Get-CimInstance Win32_UserProfile -Filter "SID='$svcSid'" -ErrorAction SilentlyContinue
                if (-not $prof -and (Test-Do "profile of '$ServiceUser'" 'Create (one hidden logon)')) {
                    if (-not $pw) { $pw = New-DRPassword; Set-LocalUser -Name $ServiceUser -Password $pw }
                    $prof = New-DRServiceProfile $svcSid $pw
                    if (-not $prof) { Stop-DR "Windows did not create a profile for '$ServiceUser'." }
                }
                if ($prof) { $svcProfile = $prof.LocalPath; Write-DROk "profile: $svcProfile" }
                if ($InteractiveSession) {
                    if (Test-Do "'$ServiceUser'" 'Sign in automatically at boot (password as an LSA secret)') {
                        if (-not $pw) { $pw = New-DRPassword; Set-LocalUser -Name $ServiceUser -Password $pw }
                        Set-DRAutoLogon $pw
                        Write-DROk "auto-logon for '$ServiceUser' configured"
                    }
                } else {
                    $wl = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' -ErrorAction SilentlyContinue
                    if ($wl.AutoAdminLogon -eq '1' -and $wl.DefaultUserName -eq $ServiceUser -and (Test-Do 'auto-logon' "Turn off (left over from an -InteractiveSession install)")) {
                        Set-DRAutoLogon $null
                        Write-DROk 'auto-logon turned off'
                    }
                }
            } elseif (-not $IsWhatIf) { Stop-DR "The account '$ServiceUser' could not be resolved." }
        } finally { $pw = $null }
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep $(if ($Public) { 'Gateway certificate' } else { 'Gateway certificate (not used in private mode)' })
    # ---------------------------------------------------------------------------------------------
    $Fingerprint = ''
    if ($Public) {
        $haveCert = (Test-Path -LiteralPath $P.GatewayCert) -and (Test-Path -LiteralPath $P.GatewayKey)
        $restoreCert = $null
        if ($restore -and (Test-Path -LiteralPath (Join-Path $restore 'secrets\tls\gateway-cert.pem')) -and (Test-Path -LiteralPath (Join-Path $restore 'secrets\tls\gateway-key.pem'))) { $restoreCert = Join-Path $restore 'secrets\tls' }
        if ($haveCert -and -not $NewCertificate) {
            $Fingerprint = Get-DRCertFingerprint $P.GatewayCert
            Write-DROk "certificate kept (fingerprint $Fingerprint)"
        } elseif ($restoreCert -and -not $NewCertificate) {
            if (Test-Do $P.Tls 'Restore the gateway certificate from the backup (existing invites stay valid)') {
                Copy-Item -LiteralPath (Join-Path $restoreCert 'gateway-cert.pem') -Destination $P.GatewayCert -Force
                Copy-Item -LiteralPath (Join-Path $restoreCert 'gateway-key.pem') -Destination $P.GatewayKey -Force
                $Fingerprint = Get-DRCertFingerprint $P.GatewayCert
                Write-DROk "certificate restored from the backup (fingerprint $Fingerprint)"
            }
        } else {
            $makeCert = Join-Path $P.App 'UndauntedGateway\tools\make-cert.js'
            if ($haveCert -and $NewCertificate) { Write-DRWarn 'a NEW certificate replaces the old one: every invite handed out so far stops working' }
            if (-not (Test-Path -LiteralPath $makeCert)) {
                if ($IsWhatIf) { Write-DRInfo "(what-if: a certificate for $PubHost would be made with the gateway's make-cert tool)" }
                else { Stop-DR "The gateway's make-cert tool is missing ($makeCert)." }
            } elseif (Test-Do $P.Tls "Create a self-signed certificate for $PubHost (10 years)") {
                $tmpCert = "$($P.GatewayCert).new"; $tmpKey = "$($P.GatewayKey).new"
                Remove-Item -LiteralPath $tmpCert, $tmpKey -Force -ErrorAction SilentlyContinue
                $code = Invoke-DRNative -FilePath $NodeExe -Arguments @($makeCert, '--host', $PubHost, '--cert', $tmpCert, '--key', $tmpKey) `
                    -WorkingDirectory (Join-Path $P.App 'UndauntedGateway') -LogBase (Join-Path $P.Logs 'install\make-cert')
                if ($code -ne 0 -or -not (Test-Path -LiteralPath $tmpCert) -or -not (Test-Path -LiteralPath $tmpKey)) {
                    Show-DRLogTail (Join-Path $P.Logs 'install\make-cert'); Stop-DR "make-cert failed (exit code $code)"
                }
                $Fingerprint = Get-DRCertFingerprint $tmpCert
                $printed = (Get-Content -LiteralPath (Join-Path $P.Logs 'install\make-cert.out.log') -Raw) -match $Fingerprint
                if (-not $printed) { Write-DRWarn 'make-cert printed a different or no fingerprint; using the one computed from the certificate file' }
                Move-Item -LiteralPath $tmpCert -Destination $P.GatewayCert -Force
                Move-Item -LiteralPath $tmpKey -Destination $P.GatewayKey -Force
                Write-DROk "certificate created (fingerprint $Fingerprint)"
            }
        }
        if (Test-Path -LiteralPath $P.GatewayCert) {
            $ci = Get-DRCertInfo $P.GatewayCert
            if ($ci) {
                Write-DRInfo "certificate: $($ci.Algorithm), valid until $($ci.NotAfter.ToString('yyyy-MM-dd')), names: $($ci.San)"
                if ($ci.San -and $ci.San -notmatch [regex]::Escape($PubHost)) { Write-DRInfo "(its names do not include $PubHost; that is fine: clients check only the fingerprint)" }
            }
        }
    } else { Write-DRSkip 'private mode' }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Keys and configuration'
    # ---------------------------------------------------------------------------------------------
    $fwd = { param($p) ($p -replace '\\', '/') }
    $oldMeta = Read-DREnv $P.MetaEnv
    $oldDeploy = Read-DREnv $P.DeployEnv
    $oldContent = Read-DREnv $P.ContentEnv
    $oldGateway = Read-DREnv $P.GatewayEnv
    $oldAllow = Read-DREnv $P.AllowlistEnv
    $rMeta = [ordered]@{}; $rDeploy = [ordered]@{}
    if ($restore) { $rMeta = Read-DREnv (Join-Path $restore 'secrets\metagame.env'); $rDeploy = Read-DREnv (Join-Path $restore 'secrets\deployserver.env') }

    # Keep every setting the old install or the backup had (progression switches and so on); the keys
    # below that describe this host are always set.
    $meta = [ordered]@{}
    foreach ($src in $oldMeta, $rMeta) { foreach ($k in $src.Keys) { $meta[$k] = $src[$k] } }
    $deploy = [ordered]@{}
    foreach ($src in $oldDeploy, $rDeploy) { foreach ($k in $src.Keys) { $deploy[$k] = $src[$k] } }
    $content = [ordered]@{}
    foreach ($k in $oldContent.Keys) { $content[$k] = $oldContent[$k] }

    # Signing keys: from the backup, else the existing install, else new ones.
    $haveSigning = $meta.Contains('AUTH_SIGNING_PRIVKEY_B64') -and $meta.Contains('AUTH_SIGNING_PUBKEY_B64') -and $meta['AUTH_SIGNING_PRIVKEY_B64'] -and $meta['AUTH_SIGNING_PUBKEY_B64']
    if ($haveSigning) { Write-DROk "token-signing keys kept ($(if ($restore) { 'from the backup' } else { 'existing' }))" }
    elseif (Test-Do 'token-signing keys' 'Generate a new RS256 key pair') {
        $tmp = Join-Path $P.Keys 'signing.tmp'
        try {
            $code = Invoke-DRNative -FilePath $NodeExe -Arguments @((Join-Path $P.Bin 'lib\dr-keys.js'), 'signing', $tmp) -LogBase (Join-Path $P.Logs 'install\keys')
            if ($code -ne 0) { Stop-DR 'Generating the signing keys failed.' }
            $gen = Read-DREnv $tmp
            $meta['AUTH_SIGNING_PRIVKEY_B64'] = $gen['AUTH_SIGNING_PRIVKEY_B64']
            $meta['AUTH_SIGNING_PUBKEY_B64'] = $gen['AUTH_SIGNING_PUBKEY_B64']
        } finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
        Write-DROk 'new token-signing keys generated'
    }

    # Game-server key: from the backup, else the existing key file, else new.
    $gsKey = $null
    if ($restore -and $rDeploy['METAGAME_API_KEY']) { $gsKey = $rDeploy['METAGAME_API_KEY'] }
    elseif ($restore -and (Test-Path -LiteralPath (Join-Path $restore 'secrets\gameserver.key'))) { $gsKey = ([IO.File]::ReadAllText((Join-Path $restore 'secrets\gameserver.key'))).Trim() }
    elseif (Test-Path -LiteralPath $P.GsKey) { $gsKey = ([IO.File]::ReadAllText($P.GsKey)).Trim() }
    elseif ($deploy['METAGAME_API_KEY']) { $gsKey = $deploy['METAGAME_API_KEY'] }
    if (-not $gsKey) { $gsKey = New-DRHexSecret 24; $gsNote = 'new game-server key generated' } else { $gsNote = 'game-server key kept' }
    if (Test-Do $P.GsKey 'Write the game-server key') {
        $cur = if (Test-Path -LiteralPath $P.GsKey) { ([IO.File]::ReadAllText($P.GsKey)).Trim() } else { '' }
        if ($cur -ne $gsKey) { Write-DRText -Path $P.GsKey -Text $gsKey }
        Write-DROk $gsNote
    }

    # Gateway and allowlist secrets: kept across re-runs, new on a fresh install (they never leave
    # this machine, so a restore does not need them).
    $gwSecret = if ($meta['GATEWAY_SECRET'] -and $oldGateway['GATEWAY_SECRET'] -eq $meta['GATEWAY_SECRET']) { $meta['GATEWAY_SECRET'] } elseif ($oldGateway['GATEWAY_SECRET']) { $oldGateway['GATEWAY_SECRET'] } else { New-DRHexSecret 32 }
    $alSecret = if ($oldAllow['ALLOWLIST_SECRET']) { $oldAllow['ALLOWLIST_SECRET'] } elseif ($oldGateway['ALLOWLIST_SECRET']) { $oldGateway['ALLOWLIST_SECRET'] } else { New-DRHexSecret 32 }

    $components = @('metagame')
    if ($contentAvailable) { $components += 'content' }
    if ($Public) { $components += 'gateway' }
    if (-not $Sandbox) { $components += 'deploy' }
    if ($Public) { $components = @('allowlist') + $components }

    $metaAddr = "${BindIp}:$($ports.metagame)"
    $meta['PORT'] = "$($ports.metagame)"
    $meta['BIND_HOST'] = $BindIp
    $meta['AUTH_MODE'] = 'APIKEY'
    $meta['DB_FILENAME'] = & $fwd $P.Db
    $meta['BACKUP_ROOT'] = & $fwd $P.Backups
    $meta['TARGET_CHANGELIST'] = $DRPinned.Changelist
    # Public mode: every friend's launcher relays the game's traffic from its own 127.0.0.1:61000.
    $meta['QOS_TARGET_URL'] = if ($Public) { "http://127.0.0.1:$DRRelayPort/QoS" } else { "http://${BindIp}:$($ports.metagame)/QoS" }
    $meta['MATCHMAKING_MODE'] = 'DEPLOYSERVER'
    $meta['DEPLOYSERVER_URL'] = "127.0.0.1:$($ports.deploy)"
    $meta['REGISTRATION_MODE'] = 'INVITECODE'
    $meta['NODE_ENV'] = 'production'
    $meta['SERVER_NAME'] = $ServerName
    $meta['SOURCE_URL'] = $DRRepo.Url
    $meta['GIT_COMMIT'] = $commit
    $meta['BODY_LOG_FILE'] = & $fwd (Join-Path $P.Logs 'bodies.log')
    if ($Public) {
        $meta['GATEWAY_SECRET'] = $gwSecret
        # Request-body capture is a development aid; on a public server it would record what players send.
        if ($meta['LOG_BODIES'] -eq '1') { Write-DRInfo 'LOG_BODIES was on in the old settings; turned off for a public server' }
        $meta['LOG_BODIES'] = '0'
    } elseif ($meta.Contains('GATEWAY_SECRET')) { $meta.Remove('GATEWAY_SECRET') }
    # Text chat: on loopback, on the port the gateway forwards WebSocket upgrades to.
    if ($Public) { Set-DRChatEnv $meta $ChatSetting ([bool]$Sandbox) } else { $meta['CHAT'] = '0' }
    if ($components -contains 'content') { $meta['CONTENT_PORT'] = "$($ports.content)" } elseif ($meta.Contains('CONTENT_PORT')) { $meta.Remove('CONTENT_PORT') }

    $deploy['PORT'] = "$($ports.deploy)"
    $deploy['BIND_HOST'] = '127.0.0.1'   # always: the deploy server has no authentication
    $deploy['MY_IP'] = $MyIp
    $deploy['PORT_RANGE_BEGIN'] = "$UdpBegin"
    $deploy['PORT_RANGE_END'] = "$UdpEnd"
    $deploy['GAMESERVER_BINARY_PATH'] = & $fwd $exe
    $deploy['METAGAME_API_KEY'] = $gsKey
    if (-not $deploy['SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP']) { $deploy['SECONDS_TO_WAIT_BETWEEN_GAMESERVER_STARTUP'] = '10' }
    if (-not $deploy.Contains('RAMSGATE_POOL')) { $deploy['RAMSGATE_POOL'] = '1' }
    if (-not $deploy.Contains('ENABLE_DOJO')) { $deploy['ENABLE_DOJO'] = '0' }
    $deploy['NODE_ENV'] = 'production'

    $content['PORT'] = "$($ports.content)"
    $content['BIND_HOST'] = $BindIp
    $content['METAGAME_URL'] = "http://$metaAddr"
    $content['CONTENT_GAME_DIR'] = & $fwd $GameRoot
    $content['CONTENT_BRANDING_DIR'] = & $fwd $P.Branding
    $content['CONTENT_NEWS_FILE'] = & $fwd $P.News
    if ($ContentManifest) { $content['CONTENT_MANIFEST'] = & $fwd $manifest } elseif ($content.Contains('CONTENT_MANIFEST')) { $content.Remove('CONTENT_MANIFEST') }
    $content['NODE_ENV'] = 'production'

    # Gateway (contract C2) and allowlist helper (contract C3).
    $gateway = [ordered]@{}
    foreach ($k in $oldGateway.Keys) { $gateway[$k] = $oldGateway[$k] }
    $gateway['GATEWAY_BIND'] = $GatewayBind
    $gateway['GATEWAY_PORT'] = "$($ports.gateway)"
    $gateway['GATEWAY_CERT'] = & $fwd $P.GatewayCert
    $gateway['GATEWAY_KEY'] = & $fwd $P.GatewayKey
    $gateway['GATEWAY_SECRET'] = $gwSecret
    $gateway['GATEWAY_METAGAME_URL'] = "http://127.0.0.1:$($ports.metagame)"
    $gateway['GATEWAY_CONTENT_URL'] = "http://127.0.0.1:$($ports.content)"
    # WebSocket upgrades: the metagame's chat listener (CHAT_PORT, the same port). A sandbox points at a
    # spare port, never at the PC's own 61099.
    $gateway['GATEWAY_WS_URL'] = Get-DRGatewayWsUrl ([bool]$Sandbox)
    $gateway['ALLOWLIST_URL'] = "http://127.0.0.1:$($ports.allowlist)"
    $gateway['ALLOWLIST_SECRET'] = $alSecret
    $gateway['NODE_ENV'] = 'production'
    foreach ($k in 'GATEWAY_CHAT_URL', 'GATEWAY_ACCESS_LOG') { if ($gateway.Contains($k)) { $gateway.Remove($k) } }   # names from kit drafts

    $allow = [ordered]@{}
    foreach ($k in $oldAllow.Keys) { $allow[$k] = $oldAllow[$k] }
    foreach ($k in 'ALLOWLIST_RULE_NAME', 'ALLOWLIST_UDP_PORTS', 'ALLOWLIST_PROGRAM') { if ($allow.Contains($k)) { $allow.Remove($k) } }   # names from kit drafts
    $allow['ALLOWLIST_BIND'] = '127.0.0.1'
    $allow['ALLOWLIST_PORT'] = "$($ports.allowlist)"
    $allow['ALLOWLIST_SECRET'] = $alSecret
    $allow['ALLOWLIST_PORTS'] = "$UdpBegin-$UdpEnd"
    $allow['ALLOWLIST_TTL_SECONDS'] = '600'
    $allow['ALLOWLIST_AUDIT_LOG'] = & $fwd (Join-Path $P.Data 'allowlist\audit.log')
    $allow['ALLOWLIST_STATE_FILE'] = & $fwd (Join-Path $P.Data 'allowlist\state.json')
    $allow['ALLOWLIST_DRY_RUN'] = if ($AllowlistDryRun) { '1' } else { '0' }
    if ($Sandbox) { $allow['ALLOWLIST_ALLOW_PRIVATE'] = '1' } elseif ($allow.Contains('ALLOWLIST_ALLOW_PRIVATE')) { $allow.Remove('ALLOWLIST_ALLOW_PRIVATE') }
    $allow['NODE_ENV'] = 'production'

    $hdr = $script:DRSecretEnvHeader
    if (Test-Do $P.Config 'Write the .env files (values not shown)') {
        $w = @()
        if (Write-DREnv $P.MetaEnv $meta $hdr) { $w += 'metagame.env' }
        if (Write-DREnv $P.DeployEnv $deploy $hdr) { $w += 'deployserver.env' }
        if ($components -contains 'content' -and (Write-DREnv $P.ContentEnv $content @('Dauntless Revived content server - written by Install-DauntlessServer.ps1.'))) { $w += 'content.env' }
        if ($Public) {
            if (Write-DREnv $P.GatewayEnv $gateway $hdr) { $w += 'gateway.env' }
            if (Write-DREnv $P.AllowlistEnv $allow $hdr) { $w += 'allowlist.env' }
        }
        if ($w.Count) { Write-DROk "written: $($w -join ', ') (values not shown)" } else { Write-DROk 'configuration unchanged' }
        if (-not (Test-Path -LiteralPath $P.News)) { Write-DRText -Path $P.News -Text "{ `"items`": [] }`r`n"; Write-DROk "news file: $($P.News) (empty; the launcher shows its items)" }
    }
    Write-DRInfo "metagame   : $metaAddr (registration by invite code only)"
    if ($components -contains 'content') { Write-DRInfo "content    : ${BindIp}:$($ports.content) (game files for registered accounts; art pack folder $($P.Branding))" }
    Write-DRInfo "deploy     : 127.0.0.1:$($ports.deploy) (never reachable from outside this machine)"
    if ($Public) {
        Write-DRInfo "gateway    : ${GatewayBind}:$($ports.gateway) (TLS; the only public TCP port)"
        Write-DRInfo "allowlist  : 127.0.0.1:$($ports.allowlist)$(if ($AllowlistDryRun) { ' (DRY-RUN: logs the firewall changes it would make)' })"
        Write-DRInfo "chat       : 127.0.0.1:$(Get-DRChatPort ([bool]$Sandbox)) $(if ($ChatSetting -eq 'On') { 'on (the gateway forwards the game''s chat connection to it)' } else { 'off (Set-Chat.ps1 -On turns it on)' })"
    }
    Write-DRInfo "game UDP   : $UdpBegin-$UdpEnd, advertised as $MyIp"

    $cfg = [ordered]@{
        Version            = 2
        Mode               = $Mode
        ServerName         = $ServerName
        Sandbox            = [bool]$Sandbox
        Root               = $Root
        BindAddress        = $BindIp
        PublicHost         = $(if ($Public) { $PubHost } else { '' })
        PublicIp           = $(if ($Public) { $MyIp } else { '' })
        GatewayBind        = $(if ($Public) { $GatewayBind } else { '' })
        CertFingerprint    = $Fingerprint
        AdminIp            = @($AdminIp | Where-Object { $_ })
        AdvertiseHost      = $Advertise
        MagicDnsName       = $MagicDns
        TailscaleShareUrl  = $(if ($Public) { '' } elseif ($TailscaleShareUrl) { $TailscaleShareUrl } else { Get-DRConfigValue $existingCfg 'TailscaleShareUrl' '' })
        Ports              = $ports
        UdpPortBegin       = $UdpBegin
        UdpPortEnd         = $UdpEnd
        Components         = $components
        AllowlistDryRun    = [bool]$AllowlistDryRun
        GameDir            = $GameRoot
        NodePath           = $NodeExe
        ServiceUser        = ''      # set once the scheduled tasks exist (below)
        ServiceProfile     = $svcProfile
        InteractiveSession = [bool]$InteractiveSession
        PerformanceLog     = [bool](Get-DRConfigValue $existingCfg 'PerformanceLog' $true)
        Chat               = $ChatSetting
        StackTask          = $DRNames.StackTask
        AllowlistTask      = $(if ($Public) { $DRNames.AllowlistTask } else { '' })
        BackupTask         = $DRNames.BackupTask
        Source             = $sourceKind
        Ref                = $refUsed
        Commit             = $commit
        SourceUrl          = $DRRepo.Url
        FirewallChanges    = @(Get-DRConfigValue $existingCfg 'FirewallChanges' @())
        InstalledAt        = $(Get-DRConfigValue $existingCfg 'InstalledAt' (Get-Date).ToString('o'))
        UpdatedAt          = (Get-Date).ToString('o')
    }
    if ($existingCfg -and (Get-DRConfigValue $existingCfg 'ServiceUser' '') -and -not $Sandbox) { $cfg.ServiceUser = Get-DRConfigValue $existingCfg 'ServiceUser' '' }
    if (Test-Do $P.ServerJson 'Write server.json') { Save-DRConfig $Root ([pscustomobject]$cfg) }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Permissions'
    # ---------------------------------------------------------------------------------------------
    # Root, code, scripts and game: administrators and SYSTEM full control, the service account read
    # only (a compromised game server must not be able to change what administrators run).
    # data and backups: the service account may write. Keys, TLS and .env: read only for it. The
    # allowlist helper's files: administrators and SYSTEM only (the helper runs as SYSTEM).
    $applyAcls = {
        $full = @{ $SidAdministrators = 'FullControl'; $SidSystem = 'FullControl' }
        $ro = $full.Clone(); $ro[$svcSid] = 'ReadAndExecute'
        $rw = $full.Clone(); $rw[$svcSid] = 'Modify'
        $sys = $full.Clone()
        if ($Sandbox) { $ro[$svcSid] = 'FullControl'; $rw[$svcSid] = 'FullControl'; $sys[$svcSid] = 'FullControl' }   # the sandbox user must be able to delete it all
        Set-DRAcl $Root $ro
        foreach ($d in $P.Data, $P.Backups) { Set-DRAcl $d $rw }
        foreach ($d in $P.Config, $P.Keys, $P.Tls) { if (Test-Path -LiteralPath $d) { Set-DRAcl $d $ro } }
        foreach ($f in @(Get-ChildItem -LiteralPath $P.Config -File) + @(Get-ChildItem -LiteralPath $P.Keys -File) + @(Get-ChildItem -LiteralPath $P.Tls -File -ErrorAction SilentlyContinue)) { Set-DRAcl $f.FullName $ro }
        if (Test-Path -LiteralPath $P.AllowlistEnv) { Set-DRAcl $P.AllowlistEnv $sys }
        $alDir = Join-Path $P.Data 'allowlist'
        if (Test-Path -LiteralPath $alDir) { $alAcl = $full.Clone(); $alAcl[$svcSid] = $(if ($Sandbox) { 'FullControl' } else { 'ReadAndExecute' }); Set-DRAcl $alDir $alAcl }
    }
    if (-not $svcSid) { Write-DRInfo '(what-if: permissions are set once the service account exists)' }
    elseif (Test-Do $Root 'Restrict permissions to Administrators, SYSTEM and the service account') {
        & $applyAcls
        if ($GameDir -and -not $Sandbox) {
            # a game folder outside the root keeps its own permissions; the service account only needs to read it
            $gameItem = Get-Item -LiteralPath $GameRoot
            $acl = $gameItem.GetAccessControl([Security.AccessControl.AccessControlSections]::Access)
            $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier($svcSid)), 'ReadAndExecute', 'ContainerInherit, ObjectInherit', 'None', 'Allow')))
            $gameItem.SetAccessControl($acl)
        }
        Write-DROk "permissions set ($(Get-DRAclSummary $P.Keys))"
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Database and owner account'
    # ---------------------------------------------------------------------------------------------
    $metaDir = Join-Path $P.App 'UndauntedMetagame'
    if ($IsWhatIf) {
        if ($restore) { Write-DRInfo "(what-if: the database and keys would be restored from $restore)" }
        elseif (-not (Test-Path -LiteralPath $P.OwnerKey)) { Write-DRInfo "(what-if: the metagame would start once and create the owner account '$OwnerName'; its key would go to $($P.OwnerKey))" }
    } else {
        if ($restore) {
            if (Test-Path -LiteralPath $P.Db) { Write-DRWarn "a database already exists at $($P.Db); not overwriting it with the backup (delete it first to restore)" }
            else {
                $chk = Invoke-DbTool @('integrity', $metaDir, (Join-Path $restore 'undaunted.db')) 'restore-check'
                if ($chk.Code -ne 0) { Stop-DR "The backup database fails its check: $($chk.Error)" }
                Copy-Item -LiteralPath (Join-Path $restore 'undaunted.db') -Destination $P.Db
                Write-DROk "database restored ($($chk.Message))"
            }
            foreach ($k in Get-ChildItem -LiteralPath (Join-Path $restore 'secrets') -Filter *.key -File -ErrorAction SilentlyContinue) {
                $dest = Join-Path $P.Keys $k.Name
                if ($k.Name -eq 'gameserver.key') { continue }   # written above from the backup's settings
                if (Test-Path -LiteralPath $dest) { Write-DRInfo "$($k.Name) exists here; kept" }
                else { Copy-Item -LiteralPath $k.FullName -Destination $dest; Write-DROk "$($k.Name) restored (not shown)" }
            }
            if ($svcSid) { & $applyAcls }
        }
        # Start the metagame once (directly, as this administrator; the database is created and migrated).
        Write-DRInfo 'starting the metagame once ...'
        [void](Invoke-KitScript 'Stack.ps1' @('start', '-Root', $Root, '-Only', 'metagame', '-NoBackup', '-Direct'))
        $api = "http://$metaAddr/undaunted/api"
        try {
            $st = Invoke-DRHttp -Url "http://$metaAddr/dauntless-status" -TimeoutSec 10
            if ($st.Status -ne 200) { Stop-DR "The metagame did not come up (see $($P.Logs)\metagame.err.log)." }
            $gs = Invoke-DbTool @('gs-key', $metaDir, $P.Db, $P.GsKey) 'gs-key'
            if ($gs.Code -ne 0) { Stop-DR "Registering the game-server key failed: $($gs.Error)" }
            Write-DROk $gs.Message

            $ownerOk = $false
            if (Test-Path -LiteralPath $P.OwnerKey) {
                $me = Invoke-DRHttp -Url "$api/GetUserInfo" -Headers @{ 'x-undaunted-user-api-key' = ([IO.File]::ReadAllText($P.OwnerKey)).Trim() }
                if ($me.Status -eq 200 -and $me.Json.IsAdmin) { $ownerOk = $true; Write-DROk "owner account '$($me.Json.Username)' works (key in $($P.OwnerKey))" }
                elseif ($me.Status -eq 200) { Write-DRWarn "owner.key belongs to '$($me.Json.Username)', which is not an admin" }
                else { Write-DRWarn "owner.key is not accepted by this database (HTTP $($me.Status))" }
            }
            if (-not $ownerOk -and $restore) { Write-DRWarn 'no working admin key came with the backup; your existing admin account keeps working with the key you already hold' }
            if (-not $ownerOk -and -not $restore) {
                if (-not $OwnerName) { Stop-DR 'An owner username is needed (-OwnerName).' }
                if (Test-Path -LiteralPath $P.OwnerKey) { Stop-DR "$($P.OwnerKey) exists but does not work here. Move it away if you want a new owner account." }
                $boot = New-DRInviteCode
                $add = Invoke-DbTool @('add-invite', $metaDir, $P.Db, $boot) 'owner-invite'
                if ($add.Code -ne 0) { Stop-DR "Could not prepare the owner registration: $($add.Error)" }
                try {
                    $reg = Invoke-DRHttp -Method POST -Url "$api/Register" -Body @{ Username = $OwnerName; InviteCode = $boot }
                } finally { [void](Invoke-DbTool @('del-invite', $metaDir, $P.Db, $boot) 'owner-invite-cleanup') }
                if ($reg.Status -eq 409) { Stop-DR "The username '$OwnerName' is taken. Run again with another -OwnerName." }
                if ($reg.Status -ne 200 -or -not $reg.Json.UUK) { Stop-DR "Registering the owner failed (HTTP $($reg.Status) $(Get-DRErrorCode $reg))." }
                $uuk = [string]$reg.Json.UUK
                Write-DRText -Path $P.OwnerKey -Text $uuk
                if ($svcSid) { Set-DRAcl $P.OwnerKey @{ $SidAdministrators = 'FullControl'; $SidSystem = 'FullControl'; $svcSid = $(if ($Sandbox) { 'FullControl' } else { 'ReadAndExecute' }) } }
                $me = Invoke-DRHttp -Url "$api/GetUserInfo" -Headers @{ 'x-undaunted-user-api-key' = $uuk }
                $uuk = $null
                if ($me.Status -ne 200) { Stop-DR 'The new owner key does not work.' }
                $adm = Invoke-DbTool @('make-admin', $metaDir, $P.Db, [string]$me.Json.UserId) 'owner-admin'
                if ($adm.Code -ne 0) { Stop-DR "Making the owner an admin failed: $($adm.Error)" }
                Write-DROk "owner account '$OwnerName' created as admin; key saved to $($P.OwnerKey) (not shown)"
            }
        } finally {
            [void](Invoke-KitScript 'Stack.ps1' @('stop', '-Root', $Root, '-NoBackup', '-Direct'))
        }
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Game config of the service account'
    # ---------------------------------------------------------------------------------------------
    # Game servers read their backend endpoints from the user Game.ini (the DLL rewrites them only in
    # client mode), so the account that runs them needs the 167 overrides pointing at this metagame.
    # Game servers run on this machine, so in public mode they talk to the metagame on 127.0.0.1.
    $cfgDir = $null
    if ($Sandbox) { $cfgDir = Join-Path $P.Data 'sandbox-profile\AppData\Local\Archon\Saved\Config\WindowsClient' }
    elseif ($svcProfile) { $cfgDir = Join-Path $svcProfile 'AppData\Local\Archon\Saved\Config\WindowsClient' }
    $dllMain = Join-Path $P.App 'native\internal-server\dllmain.cpp'
    if (-not $cfgDir -or -not (Test-Path -LiteralPath $dllMain)) { Write-DRInfo '(what-if: Game.ini and Engine.ini would be written for the service account)' }
    elseif (Test-Do $cfgDir 'Write Game.ini (endpoint overrides) and Engine.ini (memory caps, chat override)') {
        $n = Write-DRGameUserConfig -ConfigDir $cfgDir -DllMainCpp $dllMain -Metagame $metaAddr
        if ($n -ne 167) { Write-DRWarn "Game.ini has $n endpoint overrides (167 expected for 1.4.4)" } else { Write-DROk "Game.ini: 167 endpoint overrides -> $metaAddr" }
        Write-DROk "Engine.ini: memory caps, chat pointed at 127.0.0.1:$DRChatPort (never Epic)"
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Firewall'
    # ---------------------------------------------------------------------------------------------
    $fwChanges = New-Object System.Collections.Generic.List[string]
    foreach ($c in @(Get-DRConfigValue $existingCfg 'FirewallChanges' @())) { if ($c) { $fwChanges.Add([string]$c) } }
    if ($Sandbox) { Write-DRSkip 'sandbox: firewall unchanged (everything listens on 127.0.0.1)' }
    elseif ($Public) {
        # 1. Our own rules: the gateway port, and the allowlist rule (kept if it exists: the helper owns its addresses).
        if (Test-Do "Windows Firewall group '$($DRNames.FirewallGroup)'" "Allow TCP $($ports.gateway) (gateway) from anywhere; UDP $UdpBegin-$UdpEnd only through the allowlist rule") {
            Get-NetFirewallRule -Group $DRNames.FirewallGroup -ErrorAction SilentlyContinue | Where-Object { $_.Name -ne $DRNames.AllowlistRuleName } | Remove-NetFirewallRule
            New-NetFirewallRule -Group $DRNames.FirewallGroup -DisplayName "Dauntless Revived - gateway (TCP $($ports.gateway))" -Direction Inbound -Action Allow -Profile Any `
                -Protocol TCP -LocalPort $ports.gateway -Program $NodeExe -Enabled True | Out-Null
            Write-DROk "inbound TCP $($ports.gateway) (node.exe, the gateway) from anywhere"
            # The helper keeps this rule's addresses and enabled state; only the game exe may receive.
            $al = Get-NetFirewallRule -Name $DRNames.AllowlistRuleName -ErrorAction SilentlyContinue
            if ($al) {
                Set-NetFirewallRule -Name $DRNames.AllowlistRuleName -Direction Inbound -Action Allow -Profile Any -Protocol UDP -LocalPort "$UdpBegin-$UdpEnd" -Program $exe
                Write-DROk "allowlist rule kept (enabled: $($al.Enabled); its addresses belong to the allowlist helper)"
            } else {
                # Created disabled, with a documentation-only address (TEST-NET-1) until the helper sets the live list.
                New-NetFirewallRule -Name $DRNames.AllowlistRuleName -Group $DRNames.FirewallGroup -DisplayName $DRNames.AllowlistRule -Direction Inbound -Action Allow -Profile Any `
                    -Protocol UDP -LocalPort "$UdpBegin-$UdpEnd" -Program $exe -RemoteAddress '192.0.2.1' -Enabled False | Out-Null
                Write-DROk "allowlist rule created, disabled (UDP $UdpBegin-$UdpEnd opens only for players who logged in)"
            }
            # A rule with the same display name but another Name would be one the helper does not own.
            foreach ($dup in @(Get-NetFirewallRule -DisplayName $DRNames.AllowlistRule -ErrorAction SilentlyContinue | Where-Object { $_.Name -ne $DRNames.AllowlistRuleName })) {
                Remove-NetFirewallRule -Name $dup.Name
                Write-DROk "removed a stray rule named like the allowlist rule ($($dup.Name))"
            }
            Write-DROk "no rule for TCP $($ports.metagame), $($ports.content), $($ports.deploy), $($ports.allowlist) (loopback only)"
        }
        # 2. SSH stays reachable (key-only is checked, not changed).
        $sshd = Get-Service -Name sshd -ErrorAction SilentlyContinue
        $sshRules = @(Get-DRRulesForPort 22)
        if ($sshd) {
            if (-not $sshRules.Count -and (Test-Do 'SSH (TCP 22)' 'Allow inbound (key-only SSH administration)')) {
                New-NetFirewallRule -Group $DRNames.FirewallGroup -DisplayName 'Dauntless Revived - SSH (TCP 22)' -Direction Inbound -Action Allow -Profile Any -Protocol TCP -LocalPort 22 -Enabled True | Out-Null
                Write-DROk 'inbound TCP 22 (SSH) allowed'
            } else { Write-DROk "SSH (TCP 22) stays open: $(@($sshRules | ForEach-Object { $_.DisplayName }) -join ', ')" }
            $sshdConfig = Join-Path $env:ProgramData 'ssh\sshd_config'
            if (Test-Path -LiteralPath $sshdConfig) {
                $pwAuth = @(Get-Content -LiteralPath $sshdConfig | Where-Object { $_ -match '^\s*PasswordAuthentication\s+(\S+)' } | ForEach-Object { $Matches[1] }) | Select-Object -First 1
                # Deploy-Remote runs the installer over a key-only SSH session (BatchMode, no password),
                # so if we are inside that session key login is already proven and password login can go
                # off safely. From the console or Remote Desktop we only warn: turning it off blindly could
                # lock out someone who has not set up a key yet.
                $overSsh = [bool]$env:SSH_CONNECTION
                if ($pwAuth -eq 'no') { Write-DROk 'SSH accepts keys only (PasswordAuthentication no)' }
                elseif ($overSsh -and (Test-Do 'SSH password login' 'Turn off (this key-only SSH session proves key login works)')) {
                    if (Disable-DRSshPasswords $sshdConfig) {
                        try { Restart-Service sshd -ErrorAction Stop; Write-DROk 'SSH password login turned off (PasswordAuthentication no); sshd restarted (this session stays up)' }
                        catch { Write-DRWarn "set PasswordAuthentication no in $sshdConfig, but could not restart sshd ($($_.Exception.Message)); run: Restart-Service sshd" }
                    } else { Write-DROk 'SSH accepts keys only (PasswordAuthentication no)' }
                } else {
                    Write-DRWarn "SSH still accepts passwords (sshd_config: PasswordAuthentication $(if ($pwAuth) { $pwAuth } else { 'not set = yes' })). Once key login works, set"
                    Write-DRInfo "   PasswordAuthentication no   in $sshdConfig, then: Restart-Service sshd"
                }
            }
        } else { Write-DRInfo 'no OpenSSH server installed (Deploy-Remote.ps1 needs it)' }
        # 2b. Account lockout, first install only (a re-run leaves whatever policy is set): slows brute
        #     force against any account that still accepts a password.
        if (-not $existingCfg -and (Test-Do 'account lockout policy' 'Lock an account for 15 min after 10 wrong passwords')) {
            $code = Invoke-DRNative -FilePath (Join-Path $sys32 'net.exe') -Arguments @('accounts', '/lockoutthreshold:10', '/lockoutwindow:15', '/lockoutduration:15') -LogBase (Join-Path $P.Logs 'install\lockout')
            if ($code -eq 0) { Write-DROk 'account lockout: 10 wrong passwords locks the account for 15 minutes' }
            else { Write-DRWarn 'could not set the account lockout policy (set it by hand: net accounts /lockoutthreshold:10)' }
        }
        # 3. Remote Desktop: only from -AdminIp.
        $rdpRules = @(Get-DRRulesForPort 3389 'TCP') + @(Get-DRRulesForPort 3389 'UDP') | Sort-Object Name -Unique
        $rdpOpen = @($rdpRules | Where-Object { @(($_ | Get-NetFirewallAddressFilter).RemoteAddress) -contains 'Any' })
        if ($AdminIp) {
            foreach ($r in $rdpRules) {
                $now = @(($r | Get-NetFirewallAddressFilter).RemoteAddress)
                if ((Compare-Object $now @($AdminIp)) -and (Test-Do "RDP rule '$($r.DisplayName)'" "Allow only from $($AdminIp -join ', ')")) {
                    $fwChanges.Add("rdp|$($r.Name)|$($now -join ',')")
                    Set-NetFirewallRule -Name $r.Name -RemoteAddress $AdminIp
                    Write-DROk "RDP rule '$($r.DisplayName)': only from $($AdminIp -join ', ') (was: $($now -join ', '))"
                }
            }
            if (-not $rdpRules.Count) { Write-DROk 'no inbound RDP rule is enabled' }
        } elseif ($rdpOpen.Count) {
            if ($KeepRdpOpen) {
                Write-Host ''
                Write-Host '   !!!!! Remote Desktop (TCP 3389) is open to the whole internet (-KeepRdpOpen) !!!!!' -ForegroundColor Red
                Write-Host "   Rules: $(($rdpOpen | ForEach-Object { $_.DisplayName }) -join ', ')" -ForegroundColor Red
                Write-Host '   Password login for Administrator over RDP is the main way public servers are taken over.' -ForegroundColor Red
                Write-Host '   Prefer -AdminIp <your own public IP>, or turn RDP off and use key-only SSH.' -ForegroundColor Red
                Write-Host ''
            } else {
                # No -AdminIp: disable the internet-open RDP rules. The previous enabled state is recorded so
                # the uninstall notes can restore it. -KeepRdpOpen (above) keeps them, or pass -AdminIp.
                foreach ($r in $rdpOpen) {
                    if (Test-Do "RDP rule '$($r.DisplayName)'" 'Disable (open to the whole internet; use -AdminIp or -KeepRdpOpen to keep it)') {
                        $fwChanges.Add("rdpdisabled|$($r.Name)|True")
                        Disable-NetFirewallRule -Name $r.Name
                        Write-DROk "RDP rule '$($r.DisplayName)' disabled (was open to the internet; administer over key-only SSH, or re-run with -AdminIp)"
                    }
                }
            }
        } else { Write-DROk 'Remote Desktop is not open to the internet' }
        # 4. Default deny, every profile.
        foreach ($fp in Get-NetFirewallProfile) {
            if (-not $fp.Enabled -or $fp.DefaultInboundAction -ne 'Block') {
                if (Test-Do "firewall profile $($fp.Name)" 'Turn on, block inbound by default') {
                    $fwChanges.Add("profile|$($fp.Name)|$($fp.Enabled)|$($fp.DefaultInboundAction)")
                    Set-NetFirewallProfile -Name $fp.Name -Enabled True -DefaultInboundAction Block -DefaultOutboundAction Allow
                    Write-DROk "profile $($fp.Name): on, inbound blocked by default"
                }
            } else { Write-DROk "profile $($fp.Name): on, inbound blocked by default" }
        }
        # 4b. Policy values override the local settings above: some VPS images set
        #     HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall\<Profile>\EnableFirewall = 0, which leaves the
        #     firewall OFF whatever the local profile says (every port open to the internet). Without a
        #     domain these come from no GPO, so they are removed; AllowLocalPolicyMerge = 0 goes first, or
        #     the local rules (SSH included) would be ignored once the firewall is on. The effective state
        #     (active store) is what gets checked.
        $polRoot = 'HKLM:\SOFTWARE\Policies\Microsoft\WindowsFirewall'
        foreach ($name in 'AllowLocalPolicyMerge', 'DefaultInboundAction', 'EnableFirewall') {
            foreach ($pn in 'DomainProfile', 'PrivateProfile', 'PublicProfile', 'StandardProfile') {
                $k = Join-Path $polRoot $pn
                $val = (Get-ItemProperty -LiteralPath $k -ErrorAction SilentlyContinue).$name
                if ($null -eq $val) { continue }
                $bad = switch ($name) { 'EnableFirewall' { $val -eq 0 } 'AllowLocalPolicyMerge' { $val -eq 0 } 'DefaultInboundAction' { $val -eq 0 } }   # 0 = off / ignore local rules / allow
                if ($bad -and (Test-Do "firewall policy value $pn\$name = $val" 'Remove (it overrides the local firewall settings)')) {
                    $fwChanges.Add("policy|$pn|$name|$val")
                    Remove-ItemProperty -LiteralPath $k -Name $name
                    Write-DROk "policy value $pn\$name = $val removed"
                }
            }
        }
        if (-not $IsWhatIf) {
            if (Test-DRFirewallEffective) { Write-DROk 'effective firewall (all profiles, after any policy): on, inbound blocked by default' }
            else {
                # The firewall service reads these policy values when it starts; without a GPO nothing
                # makes it read them again, so the change takes effect at the next restart.
                $FirewallNeedsRestart = $true
                $eff = (Get-NetFirewallProfile -PolicyStore ActiveStore | ForEach-Object { "$($_.Name)=$(if ($_.Enabled) { 'on' } else { 'OFF' })/$($_.DefaultInboundAction)" }) -join ', '
                Write-DRWarn "the firewall in effect is still OFF ($eff). RESTART THE SERVER (Restart-Computer) to turn it on;"
                Write-DRInfo '   until then every port on this server is open to the internet. If it is still off after a restart,'
                Write-DRInfo '   look under HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall and at the provider''s settings.'
            }
        }
        # 5. Audit: anything else that accepts connections from anywhere on the public profile.
        $known = @($sshRules + $rdpRules | ForEach-Object { $_.Name })
        $others = @(Get-NetFirewallRule -Direction Inbound -Action Allow -Enabled True -ErrorAction SilentlyContinue | Where-Object {
            $_.Group -ne $DRNames.FirewallGroup -and $known -notcontains $_.Name -and ("$($_.Profile)" -match 'Any|Public')
        } | Where-Object { @(($_ | Get-NetFirewallAddressFilter).RemoteAddress) -contains 'Any' } | Where-Object {
            $pf = $_ | Get-NetFirewallPortFilter
            $pf.Protocol -in 'TCP', 'UDP', 'Any'
        })
        if ($others.Count) {
            Write-DRWarn "$($others.Count) other inbound rule(s) accept connections from anywhere on the Public profile:"
            foreach ($o in $others | Select-Object -First 25) {
                $pf = $o | Get-NetFirewallPortFilter
                Write-DRInfo ("   {0}  ({1} {2})  -> Disable-NetFirewallRule -Name '{3}'" -f $o.DisplayName, $pf.Protocol, (@($pf.LocalPort) -join ','), $o.Name)
            }
            Write-DRInfo '   Review them; disable what you do not need.'
        } else { Write-DROk 'no other inbound TCP/UDP rule accepts connections from anywhere' }
    } else {
        foreach ($fp in Get-NetFirewallProfile) {
            if (-not $fp.Enabled -or $fp.DefaultInboundAction -eq 'Allow') {
                Write-DRWarn "firewall profile $($fp.Name): enabled=$($fp.Enabled), default inbound=$($fp.DefaultInboundAction). The rules below only limit anything if it is on and blocks by default:"
                Write-DRInfo "   Set-NetFirewallProfile -Name $($fp.Name) -Enabled True -DefaultInboundAction Block   (check your remote-desktop rule first)"
            }
        }
        if (-not (Test-DRFirewallEffective)) {
            Write-DRWarn 'the firewall actually in effect is off or allows inbound by default on some profile, whatever the local settings say: a policy overrides them'
            Write-DRInfo '   look for EnableFirewall = 0 under HKLM\SOFTWARE\Policies\Microsoft\WindowsFirewall\<Profile>'
        }
        $ts = Get-DRTailscaleAdapter
        if (Test-Do "Windows Firewall group '$($DRNames.FirewallGroup)'" 'Replace the inbound rules (Tailscale addresses only)') {
            Get-NetFirewallRule -Group $DRNames.FirewallGroup -ErrorAction SilentlyContinue | Remove-NetFirewallRule
            $common = @{ Group = $DRNames.FirewallGroup; Direction = 'Inbound'; Action = 'Allow'; Profile = 'Any'; RemoteAddress = '100.64.0.0/10'; Enabled = 'True' }
            if ($ts) { $common.InterfaceAlias = $ts.Name } else { Write-DRWarn 'no Tailscale adapter found; the rules are limited by address only' }
            New-NetFirewallRule @common -DisplayName "Dauntless Revived - metagame (TCP $($ports.metagame), Tailscale only)" -Protocol TCP -LocalPort $ports.metagame -Program $NodeExe | Out-Null
            if ($components -contains 'content') {
                New-NetFirewallRule @common -DisplayName "Dauntless Revived - content server (TCP $($ports.content), Tailscale only)" -Protocol TCP -LocalPort $ports.content -Program $NodeExe | Out-Null
            }
            New-NetFirewallRule @common -DisplayName "Dauntless Revived - game servers (UDP $UdpBegin-$UdpEnd, Tailscale only)" -Protocol UDP -LocalPort "$UdpBegin-$UdpEnd" -Program $exe | Out-Null
            Write-DROk "inbound: TCP $($ports.metagame)$(if ($components -contains 'content') { ", $($ports.content)" }) (node.exe) and UDP $UdpBegin-$UdpEnd (game exe), from 100.64.0.0/10$(if ($ts) { " on '$($ts.Name)'" }) only"
            Write-DROk "no rule for TCP $($ports.deploy) (deploy server, loopback only)"
        }
    }
    if (-not $Sandbox) {
        # Rules made elsewhere for the same programs (for example by an "allow access" prompt) could open more.
        $others = @(Get-NetFirewallApplicationFilter -Program $NodeExe, $exe -ErrorAction SilentlyContinue | Get-NetFirewallRule -ErrorAction SilentlyContinue |
            Where-Object { $_.Group -ne $DRNames.FirewallGroup -and $_.Direction -eq 'Inbound' -and $_.Enabled -eq 'True' })
        foreach ($o in $others) { Write-DRWarn "other inbound rule for node.exe or the game: '$($o.DisplayName)' ($($o.Action)). Remove it unless you need it: Remove-NetFirewallRule -Name '$($o.Name)'" }
        if ($fwChanges.Count -and -not $IsWhatIf) { $cfg.FirewallChanges = @($fwChanges | Select-Object -Unique); Save-DRConfig $Root ([pscustomobject]$cfg) }
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Scheduled tasks'
    # ---------------------------------------------------------------------------------------------
    if ($Sandbox) { Write-DRSkip 'sandbox: no scheduled tasks (start and stop with Stack.ps1)' }
    else {
        $userId = "$env:COMPUTERNAME\$ServiceUser"
        $psExe = Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'
        $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) `
            -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable -Priority 5
        # The service account's tasks store its password (see Register-DRServiceTask): a new random one
        # on every run, never shown. Its tasks are all registered again below with it.
        $taskPw = $null
        if (-not $IsWhatIf) {
            $taskPw = New-DRPassword
            Set-LocalUser -Name $ServiceUser -Password $taskPw
            if ($InteractiveSession) { Set-DRAutoLogon $taskPw }   # auto-logon keeps working with the new password
        }
        try {
            if (Test-Do "task '$($DRNames.StackTask)'" "Register (runs as $ServiceUser)") {
                $stackArgs = ConvertTo-DRArgString @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-WindowStyle', 'Hidden', '-File', (Join-Path $P.Bin 'Stack.ps1'), 'supervise', '-Root', $Root)
                $run = New-ScheduledTaskAction -Execute $psExe -Argument $stackArgs -WorkingDirectory $P.Bin
                $stackDesc = 'Starts the Dauntless Revived server and restarts crashed parts (Stack.ps1 supervise).'
                if ($InteractiveSession) {
                    $actions = @((New-ScheduledTaskAction -Execute (Join-Path $sys32 'rundll32.exe') -Argument 'user32.dll,LockWorkStation'), $run)
                    $trigger = New-ScheduledTaskTrigger -AtLogOn -User $userId
                    $principal = New-ScheduledTaskPrincipal -UserId $userId -LogonType Interactive -RunLevel Limited
                    Register-ScheduledTask -TaskName $DRNames.StackTask -Action $actions -Trigger $trigger -Principal $principal -Settings $settings -Force -Description $stackDesc | Out-Null
                } else {
                    $trigger = New-ScheduledTaskTrigger -AtStartup
                    $trigger.Delay = 'PT1M'
                    Register-DRServiceTask -TaskName $DRNames.StackTask -Action $run -Trigger $trigger -Settings $settings -Description $stackDesc -Password $taskPw
                }
                Write-DROk "'$($DRNames.StackTask)': $(if ($InteractiveSession) { "at logon of $ServiceUser (session locked right away)" } else { 'at startup, without a logon (session 0)' })"
            }
            if ($Public) {
                if (Test-Do "task '$($DRNames.AllowlistTask)'" 'Register (runs as SYSTEM with highest privileges; only the allowlist helper)') {
                    $alArgs = ConvertTo-DRArgString @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-WindowStyle', 'Hidden', '-File', (Join-Path $P.Bin 'Stack.ps1'), 'supervise', '-Root', $Root, '-Only', 'allowlist')
                    $alAction = New-ScheduledTaskAction -Execute $psExe -Argument $alArgs -WorkingDirectory $P.Bin
                    $alPrincipal = New-ScheduledTaskPrincipal -UserId 'S-1-5-18' -LogonType ServiceAccount -RunLevel Highest
                    Register-ScheduledTask -TaskName $DRNames.AllowlistTask -Action $alAction -Trigger (New-ScheduledTaskTrigger -AtStartup) -Principal $alPrincipal -Settings $settings -Force `
                        -Description 'Dauntless Revived allowlist helper: opens the game UDP ports only for players who logged in (one firewall rule).' | Out-Null
                    Write-DROk "'$($DRNames.AllowlistTask)': at startup, as SYSTEM (elevated; changes only the rule '$($DRNames.AllowlistRule)')"
                }
            } elseif (Get-ScheduledTask -TaskName $DRNames.AllowlistTask -ErrorAction SilentlyContinue) {
                if (Test-Do "task '$($DRNames.AllowlistTask)'" 'Remove (private mode has no allowlist helper)') { Unregister-ScheduledTask -TaskName $DRNames.AllowlistTask -Confirm:$false }
            }
            if (Test-Do "task '$($DRNames.BackupTask)'" "Register (hourly, runs as $ServiceUser)") {
                $bkAction = New-ScheduledTaskAction -Execute (Join-Path $sys32 'wscript.exe') -Argument (ConvertTo-DRArgString @((Join-Path $P.Bin 'backup-hidden.vbs'), $Root)) -WorkingDirectory $P.Bin
                $bkTrigger = New-ScheduledTaskTrigger -Daily -At '00:05'
                $bkTrigger.Repetition = (New-ScheduledTaskTrigger -Once -At '00:05' -RepetitionInterval (New-TimeSpan -Hours 1) -RepetitionDuration (New-TimeSpan -Hours 23 -Minutes 55)).Repetition
                $bkSettings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 30) -MultipleInstances IgnoreNew -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries -StartWhenAvailable
                Register-DRServiceTask -TaskName $DRNames.BackupTask -Action $bkAction -Trigger $bkTrigger -Settings $bkSettings -Password $taskPw `
                    -Description 'Hourly backup of the Dauntless Revived database and keys (48 hourly + 30 daily kept).'
                Write-DROk "'$($DRNames.BackupTask)': hourly"
            }
        } finally { $taskPw = $null }
        if (-not $IsWhatIf) {
            $cfg.ServiceUser = $ServiceUser
            Save-DRConfig $Root ([pscustomobject]$cfg)
        }
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Backup and start'
    # ---------------------------------------------------------------------------------------------
    if (-not $IsWhatIf) {
        $code = Invoke-KitScript 'Backup-DauntlessServer.ps1' @('-Root', $Root)
        if ($code -ne 0) { Stop-DR 'The first backup failed.' }
        if ($svcSid) { & $applyAcls }   # files created above by this administrator: re-apply
        if ($NoStart) { Write-DRSkip '-NoStart: not starting the stack' }
        elseif ($InteractiveSession) {
            Write-DRInfo "Restart the server (or sign out and let '$ServiceUser' sign in): the stack starts at that logon."
        } else {
            [void](Invoke-KitScript 'Stack.ps1' @('start', '-Root', $Root, '-NoBackup'))
        }
    }

    # ---------------------------------------------------------------------------------------------
    Write-DRStep 'Done'
    # ---------------------------------------------------------------------------------------------
    Write-Host ''
    if ($IsWhatIf) {
        Write-Host '  What-if run: nothing was changed. Run the same command without -WhatIf to install.' -ForegroundColor Green
        exit 0
    }
    if ($Public) {
        Write-Host "  Server '$ServerName' at ${PubHost}:$($ports.gateway) (public mode)$(if ($Sandbox) { '  (sandbox)' })" -ForegroundColor Green
        Write-Host "  Certificate fingerprint (goes into every invite): $Fingerprint" -ForegroundColor Green
        Write-Host "  DRFINGERPRINT=$Fingerprint"
        if (-not $Sandbox) {
            Write-Host ''
            Write-Host '  At your provider (cloud firewall / security group), allow inbound from ANY address:' -ForegroundColor Yellow
            Write-Host "     TCP $($ports.gateway)  (the gateway)   and   UDP $UdpBegin-$UdpEnd  (the game servers)" -ForegroundColor Yellow
            Write-Host "  This server's own allowlist limits UDP to logged-in players. Without the provider UDP rule," -ForegroundColor Yellow
            Write-Host '  friends log in and then hang loading Ramsgate. This server cannot test the provider firewall.' -ForegroundColor Yellow
        }
        if ($FirewallNeedsRestart) {
            Write-Host ''
            Write-Host '  !!!!! RESTART THIS SERVER NOW (Restart-Computer): the Windows firewall is not in effect yet !!!!!' -ForegroundColor Red
            Write-Host '  A policy value that kept it off was removed; the firewall service reads it again only when it starts.' -ForegroundColor Red
            Write-Host '  The server starts by itself after the restart.' -ForegroundColor Red
        }
    } else {
        Write-Host "  Server '$ServerName' at ${Advertise}:$($ports.metagame) (private mode, Tailscale)$(if ($Sandbox) { '  (sandbox)' })" -ForegroundColor Green
    }
    Write-Host "  Owner key (your login and the admin key): $($P.OwnerKey)"
    Write-Host '    Keep a copy in a password manager. It is never shown; nobody can recover it.'
    Write-Host ''
    Write-Host '  Next:'
    if ($Public) {
        Write-Host "    1. Make an invite:    $($P.Bin)\New-Invite.ps1 -For <name>"
        Write-Host "    2. See who is online: $($P.Bin)\Get-ServerStatus.ps1  (as administrator: it asks with the owner key)"
    } else {
        Write-Host "    1. Share this machine with each friend in Tailscale (admin console > Machines > $TailscaleHostname > Share)."
        Write-Host "    2. Make an invite:    $($P.Bin)\New-Invite.ps1 -For <name> [-ShareUrl <share link> -SaveShareUrl]"
        Write-Host "    3. See who is online: $($P.Bin)\Get-ServerStatus.ps1  (as administrator: it asks with the owner key)"
    }
    Write-Host "    Status / restart:     $($P.Bin)\Stack.ps1 status | restart"
    if (-not $Sandbox -and -not $InteractiveSession) {
        Write-Host ''
        Write-Host '  The stack runs without a desktop (session 0). If Ramsgate never appears on UDP 8777 after a' -ForegroundColor Yellow
        Write-Host '  minute (Stack.ps1 status), run this installer again with -InteractiveSession.' -ForegroundColor Yellow
    }
} catch {
    if ("$_" -notmatch '^DRFAIL:') { Write-Host "   FAIL  $_" -ForegroundColor Red; Write-Host "         at $($_.InvocationInfo.PositionMessage)" }
    $exitCode = 1
}
exit $exitCode
