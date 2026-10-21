<#
.SYNOPSIS
    End-to-end test of the Windows Server kit on a development PC, in -Sandbox mode (no system
    changes): installs a public-mode server into a scratch folder, starts the metagame, content server,
    gateway and allowlist helper (dry-run) on spare loopback ports, makes an invite and checks the whole
    public path through the gateway with the pinned certificate, takes a performance sample, then
    restores a second install from the first one's backup, stops everything and deletes the scratch
    folder.

    Never used: ports 61000-61099, the game, the firewall, scheduled tasks, accounts, certificate stores.
    Ports: metagame 62000, content 62002, allowlist 62005, gateway 62443, chat 62099 (deploy 62001 stays
    unused). Chat is installed on, checked through the gateway, then switched off with Set-Chat.ps1.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-Sandbox.ps1
#>
param(
    [string]$SandboxDir = 'C:\dr\sandbox-ws2019',
    # The service binaries to install (a cargo target folder or a CI server-binaries download).
    # Default: rust\target\debug of this checkout, else rust\target\release.
    [string]$ServerBinaries,
    [switch]$KeepSandbox,
    [switch]$SkipRestore
)
$ErrorActionPreference = 'Stop'
$Kit = Split-Path $PSScriptRoot -Parent
. (Join-Path $Kit 'DauntlessServer.Common.ps1')

if ((Split-Path $SandboxDir -Leaf) -notmatch 'sandbox') { throw "-SandboxDir must be a folder whose name contains 'sandbox' (it is deleted at the end)." }
$Root = Join-Path $SandboxDir 'root'
$Root2 = Join-Path $SandboxDir 'root-restored'
$InputDir = Join-Path $SandboxDir 'input'
$Log = Join-Path $SandboxDir 'test.log'

$script:Pass = 0; $script:Fail = 0
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
    if ($Ok) { $script:Pass++; Write-Host "  ok    $Name" -ForegroundColor Green }
    else { $script:Fail++; Write-Host "  FAIL  $Name  $Detail" -ForegroundColor Red }
}
function Step([string]$Text) { Write-Host ''; Write-Host "== $Text" -ForegroundColor Cyan }
function Run-Kit([string]$Script, [string[]]$Arguments) {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try {
        $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $Script @Arguments 2>&1 | ForEach-Object { "$_" }
        $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $old }
    Add-Content -LiteralPath $Log -Value (@("----- $(Split-Path $Script -Leaf) $($Arguments -join ' ') (exit $code)") + $out)
    return [pscustomobject]@{ Code = $code; Out = $out; Text = ($out -join "`n") }
}
# The game's chat connection as libwebsockets sends it (GET //, protocol xmpp), over TLS pinned to the
# gateway's fingerprint. Returns the HTTP status of the answer (101 when chat is on), 0 or -1 on failure.
function Get-ChatUpgradeStatus([int]$Port, [string]$Fingerprint) {
    $client = $null
    try {
        $client = New-Object Net.Sockets.TcpClient('127.0.0.1', $Port)
        $pinned = [Net.Security.RemoteCertificateValidationCallback]{
            param($sender, $cert, $chain, $errors)
            $sha = [Security.Cryptography.SHA256]::Create()
            try { return ((ConvertTo-DRHex $sha.ComputeHash($cert.GetRawCertData())) -eq $Fingerprint) } finally { $sha.Dispose() }
        }
        $ssl = New-Object Net.Security.SslStream($client.GetStream(), $false, $pinned)
        $ssl.AuthenticateAsClient('127.0.0.1', $null, [Security.Authentication.SslProtocols]::Tls12, $false)
        $key = [Convert]::ToBase64String((Get-DRRandomBytes 16))
        $req = "GET // HTTP/1.1`r`nPragma: no-cache`r`nCache-Control: no-cache`r`nHost: 127.0.0.1`r`nOrigin: http://127.0.0.1`r`nUpgrade: websocket`r`nConnection: Upgrade`r`nSec-WebSocket-Key: $key`r`nSec-WebSocket-Protocol: xmpp`r`nSec-WebSocket-Version: 13`r`n`r`n"
        $bytes = [Text.Encoding]::ASCII.GetBytes($req)
        $ssl.Write($bytes, 0, $bytes.Length)
        $ssl.Flush()
        $buf = New-Object byte[] 2048
        $n = $ssl.Read($buf, 0, $buf.Length)
        $head = [Text.Encoding]::ASCII.GetString($buf, 0, $n)
        if ($head -match '^HTTP/1\.1 (\d{3})') { return [int]$Matches[1] }
        return 0
    } catch { return -1 } finally { if ($client) { $client.Close() } }
}
function Get-LivePids { foreach ($p in 61000, 61001) { $c = Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1; if ($c) { "$p=$($c.OwningProcess)" } else { "$p=down" } } }
# The sandbox's services: the binaries under its root (and, from an old Node install, node.exe
# processes whose command line names it).
function Get-SandboxNodes([string]$Under) {
    @(Get-CimInstance Win32_Process | Where-Object {
        ($_.ExecutablePath -and $_.ExecutablePath.StartsWith($Under, [StringComparison]::OrdinalIgnoreCase)) -or
        ($_.Name -eq 'node.exe' -and $_.CommandLine -and $_.CommandLine.IndexOf($Under, [StringComparison]::OrdinalIgnoreCase) -ge 0)
    })
}
function Stop-Sandbox([string]$R) {
    if (Test-Path -LiteralPath (Join-Path $R 'data\config\server.json')) { [void](Run-Kit (Join-Path $R 'bin\Stack.ps1') @('stop', '-Root', $R, '-NoBackup')) }
    foreach ($p in Get-SandboxNodes $R) { Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue }
}

$livePids = (Get-LivePids) -join ' '
Write-Host "live stack before: $livePids"
if (Test-Path -LiteralPath $SandboxDir) { Stop-Sandbox $Root; Stop-Sandbox $Root2; Remove-Item -LiteralPath $SandboxDir -Recurse -Force }
foreach ($p in 62000, 62002, 62005, 62099, 62443) { if (Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue) { throw "port $p is in use; the sandbox needs it" } }
New-Item -ItemType Directory -Force -Path $InputDir | Out-Null
Set-Content -LiteralPath $Log -Value "sandbox test $(Get-Date -Format o)" -Encoding UTF8

try {
    # ------------------------------------------------------------------------------------------
    Step 'A stand-in game folder and its manifest'
    $game = Join-Path $InputDir 'game'
    $files = [ordered]@{
        'Archon/Binaries/Win64/Dauntless-Win64-Shipping.exe' = 65536
        'Archon/Content/Paks/sandbox-test.pak'              = 1048576
        'Version.txt'                                        = 0
    }
    $rng = New-Object Random 42
    $entries = @()
    $total = 0
    foreach ($rel in $files.Keys) {
        $path = Join-Path $game ($rel -replace '/', '\')
        New-Item -ItemType Directory -Force -Path (Split-Path $path -Parent) | Out-Null
        if ($rel -eq 'Version.txt') { [IO.File]::WriteAllText($path, $DRPinned.Build) }
        else { $b = New-Object byte[] $files[$rel]; $rng.NextBytes($b); [IO.File]::WriteAllBytes($path, $b) }
        $len = (Get-Item -LiteralPath $path).Length
        $entries += [ordered]@{ path = $rel; size = $len; sha256 = (Get-DRSha256 $path).ToLowerInvariant() }
        $total += $len
    }
    $manifest = Join-Path $InputDir 'manifest.json'
    Write-DRText -Path $manifest -Text (([ordered]@{ build = $DRPinned.Build; totalBytes = $total; files = $entries }) | ConvertTo-Json -Depth 4)
    Check 'stand-in game folder' (Test-Path -LiteralPath (Join-Path $game 'Archon\Binaries\Win64'))

    if (-not $ServerBinaries) {
        $repo = Split-Path (Split-Path $Kit -Parent) -Parent
        foreach ($t in 'debug', 'release') {
            $d = Join-Path $repo "rust\target\$t"
            if (Test-Path -LiteralPath (Join-Path $d 'metagame.exe')) { $ServerBinaries = $d; break }
        }
    }
    Check 'service binaries to install' ([bool]$ServerBinaries) 'build them first: cargo build --bins -p metagame -p gateway -p content -p deploy (in rust\)'
    $common = @('-Sandbox', '-Mode', 'Public', '-GameDir', $game, '-ContentManifest', $manifest, '-ServerName', 'Sandbox Ramsgate', '-PublicHost', '127.0.0.1', '-Chat', 'On', '-ServerBinaries', $ServerBinaries)

    # ------------------------------------------------------------------------------------------
    Step 'Installer -WhatIf (nothing may be created)'
    $w = Run-Kit (Join-Path $Kit 'Install-DauntlessServer.ps1') (@('-InstallRoot', $Root, '-OwnerName', 'SandboxOwner', '-WhatIf') + $common)
    Check 'what-if run exits 0' ($w.Code -eq 0) (($w.Out | Select-Object -Last 8) -join ' | ')
    Check 'what-if created nothing' (-not (Test-Path -LiteralPath $Root))
    Check 'what-if lists the code step' ($w.Text -match 'What if: Performing the operation "Copy the server code and install its service binaries')
    Check 'what-if lists the certificate step' ($w.Text -match 'certificate')

    # ------------------------------------------------------------------------------------------
    Step 'Installer, real sandbox run (builds the code; a few minutes)'
    $t0 = Get-Date
    $r = Run-Kit (Join-Path $Kit 'Install-DauntlessServer.ps1') (@('-InstallRoot', $Root, '-OwnerName', 'SandboxOwner') + $common)
    Write-Host ("  install took {0:N0} s" -f ((Get-Date) - $t0).TotalSeconds)
    Check 'install exits 0' ($r.Code -eq 0) (($r.Out | Where-Object { $_ -match 'FAIL|WARN' } | Select-Object -First 6) -join ' | ')
    if ($r.Code -ne 0) { ($r.Out | Select-Object -Last 25) | ForEach-Object { Write-Host "     $_" }; throw 'install failed' }
    $cfg = Get-Content -LiteralPath (Join-Path $Root 'data\config\server.json') -Raw | ConvertFrom-Json
    $fp = $cfg.CertFingerprint
    Check 'server.json: public mode, fingerprint' ($cfg.Mode -eq 'Public' -and (Test-DRFingerprint $fp) -and $cfg.PublicHost -eq '127.0.0.1' -and $cfg.Ports.gateway -eq 62443)
    Check 'installer printed the fingerprint' ($r.Text -match "DRFINGERPRINT=$fp")
    Check 'fingerprint = certificate file' ((Get-DRCertFingerprint (Join-Path $Root 'data\tls\gateway-cert.pem')) -eq $fp)
    $meta = Read-DREnv (Join-Path $Root 'data\config\metagame.env')
    $gwEnv = Read-DREnv (Join-Path $Root 'data\config\gateway.env')
    $alEnv = Read-DREnv (Join-Path $Root 'data\config\allowlist.env')
    $ctEnv = Read-DREnv (Join-Path $Root 'data\config\content.env')
    $dpEnv = Read-DREnv (Join-Path $Root 'data\config\deployserver.env')
    Check 'metagame on 127.0.0.1, invite-only, QoS via the relay' ($meta['BIND_HOST'] -eq '127.0.0.1' -and $meta['REGISTRATION_MODE'] -eq 'INVITECODE' -and $meta['QOS_TARGET_URL'] -eq 'http://127.0.0.1:61000/QoS' -and $meta['PORT'] -eq '62000')
    Check 'gateway secret shared by metagame and gateway (not printed)' ($meta['GATEWAY_SECRET'] -and $meta['GATEWAY_SECRET'] -eq $gwEnv['GATEWAY_SECRET'] -and $meta['GATEWAY_SECRET'].Length -ge 32)
    Check 'allowlist secret shared by gateway and helper (not printed)' ($alEnv['ALLOWLIST_SECRET'] -and $alEnv['ALLOWLIST_SECRET'] -eq $gwEnv['ALLOWLIST_SECRET'] -and $alEnv['ALLOWLIST_SECRET'] -ne $meta['GATEWAY_SECRET'])
    Check 'gateway binds 127.0.0.1 in the sandbox' ($gwEnv['GATEWAY_BIND'] -eq '127.0.0.1' -and $gwEnv['GATEWAY_PORT'] -eq '62443' -and $gwEnv['GATEWAY_METAGAME_URL'] -eq 'http://127.0.0.1:62000' -and $gwEnv['GATEWAY_CONTENT_URL'] -eq 'http://127.0.0.1:62002')
    Check 'gateway WebSocket upstream on a spare port' ($gwEnv['GATEWAY_WS_URL'] -eq 'http://127.0.0.1:62099')
    Check 'allowlist helper in dry-run' ($alEnv['ALLOWLIST_DRY_RUN'] -eq '1' -and $alEnv['ALLOWLIST_PORT'] -eq '62005' -and $alEnv['ALLOWLIST_PORTS'] -eq '8770-8777')
    Check 'content and deploy server on 127.0.0.1' ($ctEnv['BIND_HOST'] -eq '127.0.0.1' -and $dpEnv['BIND_HOST'] -eq '127.0.0.1' -and $dpEnv['MY_IP'] -eq '127.0.0.1')
    Check 'body capture off on a public server' ($meta['LOG_BODIES'] -eq '0')
    Check 'owner key written' (Test-Path -LiteralPath (Join-Path $Root 'data\keys\owner.key'))
    Check 'Game.ini with 167 endpoints' (@(Get-Content -LiteralPath (Join-Path $Root 'data\sandbox-profile\AppData\Local\Archon\Saved\Config\WindowsClient\Game.ini') | Where-Object { $_ -match '^\w+="http://127\.0\.0\.1:62000' }).Count -eq 167)
    foreach ($d in $DRPinned.Dlls.Keys) { Check "$d pinned in the game folder" ((Get-DRSha256 (Join-Path $game "Archon\Binaries\Win64\$d")) -eq $DRPinned.Dlls[$d]) }
    Check 'a first backup exists' (@(Get-DRBackupFolders (Join-Path $Root 'backups')).Count -ge 1)
    foreach ($c in 'allowlist', 'metagame', 'content', 'gateway') { Check "$c is running" (@(Get-DRComponentProcesses (Get-DRPaths $Root) $c).Count -eq 1) }
    Check 'no deploy server in the sandbox' (@(Get-DRComponentProcesses (Get-DRPaths $Root) 'deploy').Count -eq 0 -and -not (Get-NetTCPConnection -LocalPort 62001 -State Listen -ErrorAction SilentlyContinue))
    $gwListen = @(Get-NetTCPConnection -LocalPort 62443 -State Listen -ErrorAction SilentlyContinue)
    Check 'gateway listens on loopback only' ($gwListen.Count -ge 1 -and -not ($gwListen | Where-Object { $_.LocalAddress -ne '127.0.0.1' }))
    Check 'chat on: CHAT=1 on 127.0.0.1, the gateway''s WebSocket port, kept in server.json' ($meta['CHAT'] -eq '1' -and $meta['CHAT_BIND_HOST'] -eq '127.0.0.1' -and $meta['CHAT_PORT'] -eq '62099' -and $cfg.Chat -eq 'On')
    $chatListen = @(Get-NetTCPConnection -LocalPort 62099 -State Listen -ErrorAction SilentlyContinue)
    Check 'chat listens on 127.0.0.1:62099 only' ($chatListen.Count -ge 1 -and -not ($chatListen | Where-Object { $_.LocalAddress -ne '127.0.0.1' }))
    $fwRules = @(Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -match 'Dauntless' }).Count

    # ------------------------------------------------------------------------------------------
    Step 'Invite and status through the gateway (TLS pinned)'
    $bin = Join-Path $Root 'bin'
    $inv = Run-Kit (Join-Path $bin 'New-Invite.ps1') @('-Root', $Root, '-For', 'Sandbox friend')
    Check 'New-Invite exits 0' ($inv.Code -eq 0) (($inv.Out | Select-Object -Last 6) -join ' | ')
    $invite = @($inv.Out | Where-Object { $_ -match '^dauntless-revived://' }) | Select-Object -Last 1
    $parsed = $null
    try { $parsed = ConvertFrom-DRInviteString $invite } catch {}
    Check 'New-Invite printed a v2 invite' ($parsed -and $parsed.Version -eq 2 -and $parsed.Host -eq '127.0.0.1' -and $parsed.Port -eq 62443 -and $parsed.Fingerprint -eq $fp -and $parsed.Name -eq 'Sandbox Ramsgate') "$invite"
    Check 'New-Invite checked the gateway first' ($inv.Text -match 'gateway answers on port 62443')
    Check 'the owner key is not in the output' (-not ($inv.Text.Contains(([IO.File]::ReadAllText((Join-Path $Root 'data\keys\owner.key'))).Trim())))
    $st = Run-Kit (Join-Path $bin 'Get-ServerStatus.ps1') @('-Invite', $invite)
    Check 'Get-ServerStatus -Invite answers through the gateway' ($st.Code -eq 0 -and $st.Text -match 'ONLINE\s+Sandbox Ramsgate' -and $st.Text -match 'TLS pinned') (($st.Out | Select-Object -First 3) -join ' | ')
    Check 'without a key the player list is hidden (no "0 players")' ($st.Text -match 'Player list hidden' -and $st.Text -match 'pass -KeyFile' -and $st.Text -notmatch 'Players online') (($st.Out | Select-Object -Last 3) -join ' | ')
    $st2 = Run-Kit (Join-Path $bin 'Get-ServerStatus.ps1') @('-Root', $Root)
    Check 'Get-ServerStatus on the server goes through the gateway' ($st2.Code -eq 0 -and $st2.Text -match '127\.0\.0\.1:62443, TLS pinned')
    Check 'Get-ServerStatus on the server lists players with the owner key' ($st2.Text -match 'Players online: \d+' -and $st2.Text -match 'Worlds and hunts running: \d+' -and $st2.Text -notmatch 'hidden') (($st2.Out | Select-Object -Last 4) -join ' | ')
    $st2d = Run-Kit (Join-Path $bin 'Get-ServerStatus.ps1') @('-Root', $Root, '-Direct')
    Check 'Get-ServerStatus -Direct lists players with the owner key' ($st2d.Code -eq 0 -and $st2d.Text -match '127\.0\.0\.1:62000' -and $st2d.Text -match 'Players online: \d+' -and $st2d.Text -notmatch 'hidden') (($st2d.Out | Select-Object -Last 4) -join ' | ')
    $ownerKeyText = ([IO.File]::ReadAllText((Join-Path $Root 'data\keys\owner.key'))).Trim()
    Check 'Get-ServerStatus prints no owner key' (-not $st2.Text.Contains($ownerKeyText) -and -not $st2d.Text.Contains($ownerKeyText))
    $stj = Run-Kit (Join-Path $bin 'Get-ServerStatus.ps1') @('-Invite', $invite, '-Json')
    $sj = $null; try { $sj = ($stj.Out -join '') | ConvertFrom-Json } catch {}
    Check 'ServerStatus JSON over the gateway (limited without a key)' ($sj -and $sj.name -eq 'Sandbox Ramsgate' -and $sj.registration -and $sj.limited -eq $true)

    # ------------------------------------------------------------------------------------------
    Step 'Gateway policy, as a friend would meet it'
    $g = "https://127.0.0.1:62443"
    $rs = Invoke-DRHttp -Url "$g/undaunted/api/RegistrationStatus" -Fingerprint $fp
    Check 'RegistrationStatus allowed' ($rs.Status -eq 200) "$($rs.Status) $($rs.Error)"
    $ci = Invoke-DRHttp -Method POST -Url "$g/undaunted/api/CreateInvite" -Fingerprint $fp -Body @{ uses = 1 }
    Check 'CreateInvite blocked (403)' ($ci.Status -eq 403) "$($ci.Status)"
    $il = Invoke-DRHttp -Url "$g/undaunted/api/InviteCodes" -Fingerprint $fp
    Check 'InviteCodes blocked (403)' ($il.Status -eq 403) "$($il.Status)"
    $gk = Invoke-DRHttp -Url "$g/dauntless-status" -Fingerprint $fp -Headers @{ 'x-undaunted-gameserver-apikey' = 'anything' }
    Check 'game-server key header blocked (403)' ($gk.Status -eq 403) "$($gk.Status)"
    $wrong = Invoke-DRHttp -Url "$g/undaunted/api/RegistrationStatus" -Fingerprint ('0' * 64)
    Check 'another fingerprint is refused' ($wrong.Status -eq 0 -and $wrong.SeenFingerprint -eq $fp)
    $reg = Invoke-DRHttp -Method POST -Url "$g/undaunted/api/Register" -Fingerprint $fp -Body @{ Username = 'SandboxFriend'; InviteCode = $parsed.Code }
    Check 'Register with the invite through the gateway' ($reg.Status -eq 200 -and $reg.Json.UUK) "$($reg.Status) $($reg.Text)"
    $friendKey = [string]$reg.Json.UUK
    $again = Invoke-DRHttp -Method POST -Url "$g/undaunted/api/Register" -Fingerprint $fp -Body @{ Username = 'SandboxFriend2'; InviteCode = $parsed.Code }
    Check 'the one-use invite is spent' ($again.Status -eq 401) "$($again.Status)"
    $me = Invoke-DRHttp -Url "$g/undaunted/api/GetUserInfo" -Fingerprint $fp -Headers @{ 'x-undaunted-user-api-key' = $friendKey }
    Check 'GetUserInfo with the new key' ($me.Status -eq 200 -and $me.Json.Username -eq 'SandboxFriend' -and -not $me.Json.IsAdmin) "$($me.Status)"
    # A friend's own key, from their PC: -Invite with -KeyFile shows the list.
    $friendKeyFile = Join-Path $SandboxDir 'friend-account.key'
    [IO.File]::WriteAllText($friendKeyFile, $friendKey)
    $stf = Run-Kit (Join-Path $bin 'Get-ServerStatus.ps1') @('-Invite', $invite, '-KeyFile', $friendKeyFile)
    Remove-Item -LiteralPath $friendKeyFile -Force
    Check 'Get-ServerStatus -Invite -KeyFile lists players with a friend''s key' ($stf.Code -eq 0 -and $stf.Text -match 'Players online: \d+' -and $stf.Text -notmatch 'hidden' -and -not $stf.Text.Contains($friendKey)) (($stf.Out | Select-Object -Last 4) -join ' | ')
    $tok = Invoke-DRHttp -Method POST -Url "$g/account/api/oauth/token" -Fingerprint $fp -Body @{ grant_type = 'exchange_code'; exchange_code = $friendKey }
    Check 'login (oauth/token) through the gateway' ($tok.Status -eq 200 -and $tok.Json.access_token) "$($tok.Status)"
    Check 'the game''s chat connection through the gateway: 101' ((Get-ChatUpgradeStatus 62443 $fp) -eq 101)
    $man = Invoke-DRHttp -Url "$g/content/v1/manifest" -Fingerprint $fp
    Check 'content manifest through the gateway' ($man.Status -eq 200 -and @($man.Json.files).Count -eq 3) "$($man.Status)"
    $pak = Invoke-DRHttp -Url "$g/content/v1/files/Archon/Content/Paks/sandbox-test.pak" -Fingerprint $fp
    Check 'game file needs an account (401 without key)' ($pak.Status -eq 401) "$($pak.Status)"
    # A download with the key (binary: through a raw pinned request).
    Initialize-DRPin
    $pin = New-Object DRKit.Pin($fp)
    $req = [Net.HttpWebRequest]::Create("$g/content/v1/files/Archon/Content/Paks/sandbox-test.pak")
    $req.ServerCertificateValidationCallback = $pin.Callback; $req.Proxy = $null; $req.KeepAlive = $false
    $req.Headers['x-undaunted-user-api-key'] = $friendKey
    $dl = Join-Path $SandboxDir 'download.pak'
    try {
        $resp = $req.GetResponse(); $fs = [IO.File]::Create($dl); $resp.GetResponseStream().CopyTo($fs); $fs.Dispose(); $resp.Close()
        Check 'game file download through the gateway, hash matches the manifest' ((Get-DRSha256 $dl).ToLowerInvariant() -eq ($entries | Where-Object { $_.path -eq 'Archon/Content/Paks/sandbox-test.pak' }).sha256)
    } catch { Check 'game file download through the gateway' $false "$($_.Exception.Message)" }
    Start-Sleep -Seconds 4   # the allowlist helper applies changes at most every 3 s
    $audit = @(Get-Content -LiteralPath (Join-Path $Root 'data\allowlist\audit.log') -ErrorAction SilentlyContinue | ForEach-Object { try { $_ | ConvertFrom-Json } catch {} })
    Check 'login fed the allowlist helper (127.0.0.1 added)' (@($audit | Where-Object { $_.event -eq 'added' -and $_.ip -eq '127.0.0.1' }).Count -ge 1) "$(@($audit).Count) audit lines"
    Check 'dry-run: the rule change is only logged' (@($audit | Where-Object { $_.event -eq 'dry_run' -and $_.rule -match 'allowlist' -and @($_.ips) -contains '127.0.0.1' }).Count -ge 1)
    $gwOut = Get-Content -LiteralPath (Join-Path $Root 'data\logs\gateway.out.log') -Raw -ErrorAction SilentlyContinue
    Check 'gateway access log has no keys' ($gwOut -and -not $gwOut.Contains($friendKey) -and -not $gwOut.Contains($meta['GATEWAY_SECRET']))
    $friendKey = $null

    # ------------------------------------------------------------------------------------------
    Step 'Update from the checkout, then roll back (builds again)'
    $repo = Split-Path (Split-Path $Kit -Parent) -Parent
    $up = Run-Kit (Join-Path $bin 'Update-DauntlessServer.ps1') @('-Root', $Root, '-SourceDir', $repo, '-Force', '-ServerBinaries', $ServerBinaries)
    Check 'update exits 0 and checks the server through the gateway' ($up.Code -eq 0 -and $up.Text -match 'the server is up on') (($up.Out | Where-Object { $_ -match 'FAIL|WARN' } | Select-Object -First 4) -join ' | ')
    Check 'the previous build is kept for a rollback' (Test-Path -LiteralPath (Join-Path $Root 'app.prev\VERSION.json'))
    Check 'the update took a backup first' ($up.Text -match 'backup: ')
    $rb = Run-Kit (Join-Path $bin 'Update-DauntlessServer.ps1') @('-Root', $Root, '-Rollback')
    Check 'rollback exits 0' ($rb.Code -eq 0 -and $rb.Text -match 'back on') (($rb.Out | Where-Object { $_ -match 'FAIL' } | Select-Object -First 4) -join ' | ')
    $after = Invoke-DRHttp -Url "$g/undaunted/api/ServerStatus" -Fingerprint $fp
    Check 'the server answers through the gateway after the rollback' ($after.Status -eq 200)

    # ------------------------------------------------------------------------------------------
    Step 'Stack status, backup, stop'
    $ss = Run-Kit (Join-Path $bin 'Stack.ps1') @('status', '-Root', $Root)
    Check 'Stack status shows the gateway check' ($ss.Code -eq 0 -and $ss.Text -match 'gateway check\s+: TLS ok' -and $ss.Text -match 'allowlist helper : \d+ player address') (($ss.Out | Where-Object { $_ -match 'gateway|allowlist' }) -join ' | ')
    Check 'Stack status shows the chat listener' ($ss.Text -match 'chat\s+: listening 127\.0\.0\.1:62099') (($ss.Out | Where-Object { $_ -match 'chat' }) -join ' | ')
    Check 'chat survived the update and the rollback' ((Read-DREnv (Join-Path $Root 'data\config\metagame.env'))['CHAT'] -eq '1')
    $off = Run-Kit (Join-Path $bin 'Set-Chat.ps1') @('-Root', $Root, '-Off')
    Check 'Set-Chat.ps1 -Off' ($off.Code -eq 0 -and $off.Text -match 'chat off') (($off.Out | Select-Object -Last 4) -join ' | ')
    Check 'chat off: CHAT=0 and "Chat": "Off"' ((Read-DREnv (Join-Path $Root 'data\config\metagame.env'))['CHAT'] -eq '0' -and (Get-Content -LiteralPath (Join-Path $Root 'data\config\server.json') -Raw | ConvertFrom-Json).Chat -eq 'Off')
    Check 'chat off: the gateway answers the chat connection 502 again' ((Get-ChatUpgradeStatus 62443 $fp) -eq 502)
    Check 'no firewall rule was touched' (@(Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -match 'Dauntless' }).Count -eq $fwRules)
    $ss = Run-Kit (Join-Path $bin 'Stack.ps1') @('status', '-Root', $Root)
    Check 'Stack status: chat off' ($ss.Text -match 'chat\s+: off') (($ss.Out | Where-Object { $_ -match 'chat' }) -join ' | ')
    Check 'Stack status prints no secret' (-not $ss.Text.Contains($alEnv['ALLOWLIST_SECRET']) -and -not $ss.Text.Contains($meta['GATEWAY_SECRET']) -and -not $ss.Text.Contains($ownerKeyText))
    Check 'Stack status counts players with the owner key' ($ss.Text -match 'players online\s+: \d+\s+game servers listed: \d+' -and $ss.Text -notmatch 'players online\s+: hidden') (($ss.Out | Where-Object { $_ -match 'players online' }) -join ' | ')
    $pl = Run-Kit (Join-Path $bin 'Write-PerformanceLog.ps1') @('-Root', $Root, '-Once')
    $perfFile = Join-Path $Root ('data\logs\performance\performance-{0}.csv' -f [datetime]::UtcNow.ToString('yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture))
    $perfRows = @(); if (Test-Path -LiteralPath $perfFile) { $perfRows = @(Import-Csv -LiteralPath $perfFile) }
    $perfHost = $perfRows | Where-Object { $_.role -eq 'host' } | Select-Object -Last 1
    $perfMissing = @('metagame', 'content', 'gateway', 'allowlist' | Where-Object { $r = $_; -not ($perfRows | Where-Object { $_.role -eq $r }) })
    Check 'Write-PerformanceLog -Once: a host row and a row per component' ($pl.Code -eq 0 -and $perfHost -and $perfMissing.Count -eq 0) "exit $($pl.Code); missing $($perfMissing -join ','); $(($pl.Out | Select-Object -Last 3) -join ' | ')"
    $perfMeta = $perfRows | Where-Object { $_.role -eq 'metagame' } | Select-Object -Last 1
    Check 'performance log: players counted with the owner key, CPU and memory per component' ($perfHost -and $perfHost.players_online -match '^\d+$' -and $perfHost.ram_total_mb -match '^\d+$' -and $perfMeta.cpu_core_percent -match '^\d' -and $perfMeta.working_set_mb -match '^\d') (($perfRows | Select-Object -First 2 | ForEach-Object { ($_.PSObject.Properties | ForEach-Object { "$($_.Name)=$($_.Value)" }) -join ' ' }) -join ' || ')
    $perfText = if (Test-Path -LiteralPath $perfFile) { [IO.File]::ReadAllText($perfFile) } else { '' }
    Check 'performance log holds no key or secret' ($perfText -and -not $perfText.Contains($ownerKeyText) -and -not $perfText.Contains($meta['GATEWAY_SECRET']) -and -not $perfText.Contains($alEnv['ALLOWLIST_SECRET']) -and $perfText -notmatch 'SandboxOwner')
    $ssPerf = Run-Kit (Join-Path $bin 'Stack.ps1') @('status', '-Root', $Root)
    Check 'Stack status shows the last performance sample' ($ssPerf.Text -match 'performance log\s+: .*performance-\d{4}-\d\d-\d\d\.csv \(last sample \d{4}-') (($ssPerf.Out | Where-Object { $_ -match 'performance' }) -join ' | ')
    $ownerKeyText = $null
    # Without a readable owner key (a PowerShell that is not elevated): hidden, not 0.
    $okPath = Join-Path $Root 'data\keys\owner.key'
    Move-Item -LiteralPath $okPath -Destination "$okPath.away"
    try {
        $ssNoKey = Run-Kit (Join-Path $bin 'Stack.ps1') @('status', '-Root', $Root)
        $stNoKey = Run-Kit (Join-Path $bin 'Get-ServerStatus.ps1') @('-Root', $Root)
    } finally { Move-Item -LiteralPath "$okPath.away" -Destination $okPath }
    Check 'Stack status without the owner key: hidden, not 0' ($ssNoKey.Text -match 'players online\s+: hidden \(registered players only; run this elevated') (($ssNoKey.Out | Where-Object { $_ -match 'players online' }) -join ' | ')
    Check 'Get-ServerStatus without the owner key: hidden, and how to see it' ($stNoKey.Code -eq 0 -and $stNoKey.Text -match 'Player list hidden' -and $stNoKey.Text -match 'elevated PowerShell' -and $stNoKey.Text -notmatch 'Players online') (($stNoKey.Out | Select-Object -Last 3) -join ' | ')
    $bk = Run-Kit (Join-Path $bin 'Backup-DauntlessServer.ps1') @('-Root', $Root)
    $newest = Get-DRBackupFolders (Join-Path $Root 'backups') | Select-Object -First 1
    Check 'backup with database, secrets and certificate' ($bk.Code -eq 0 -and (Test-Path -LiteralPath (Join-Path $newest.FullName 'undaunted.db')) -and (Test-Path -LiteralPath (Join-Path $newest.FullName 'secrets\tls\gateway-key.pem')) -and (Test-Path -LiteralPath (Join-Path $newest.FullName 'secrets\gateway.env')) -and (Test-Path -LiteralPath (Join-Path $newest.FullName 'secrets\owner.key')))
    $stop = Run-Kit (Join-Path $bin 'Stack.ps1') @('stop', '-Root', $Root)
    Check 'Stack stop' ($stop.Code -eq 0 -and (Get-SandboxNodes $Root).Count -eq 0) (($stop.Out | Select-Object -Last 4) -join ' | ')
    Check 'sandbox ports closed again' (-not (Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $_.LocalPort -in 62000, 62002, 62005, 62099, 62443 }))

    # ------------------------------------------------------------------------------------------
    if (-not $SkipRestore) {
        Step 'A second install restored from that backup (builds again)'
        $restoreFrom = (Get-DRBackupFolders (Join-Path $Root 'backups') | Select-Object -First 1).FullName
        $r2 = Run-Kit (Join-Path $Kit 'Install-DauntlessServer.ps1') (@('-InstallRoot', $Root2, '-RestoreFrom', $restoreFrom) + $common)
        Check 'restore install exits 0' ($r2.Code -eq 0) (($r2.Out | Where-Object { $_ -match 'FAIL' } | Select-Object -First 4) -join ' | ')
        $cfg2 = Get-Content -LiteralPath (Join-Path $Root2 'data\config\server.json') -Raw | ConvertFrom-Json
        Check 'the certificate came along (same fingerprint: old invites keep working)' ($cfg2.CertFingerprint -eq $fp)
        Check 'the database came along' ($r2.Text -match 'database restored \(ok, 2 users\)')
        Check 'the owner key works on the restored server' ($r2.Text -match "owner account 'SandboxOwner' works")
        $meta2 = Read-DREnv (Join-Path $Root2 'data\config\metagame.env')
        Check 'signing keys came along' ($meta2['AUTH_SIGNING_PUBKEY_B64'] -eq $meta['AUTH_SIGNING_PUBKEY_B64'])
        $st3 = Run-Kit (Join-Path $Root2 'bin\Get-ServerStatus.ps1') @('-Invite', $invite)
        Check 'the old invite string reaches the restored server' ($st3.Code -eq 0 -and $st3.Text -match 'ONLINE')
        $stop2 = Run-Kit (Join-Path $Root2 'bin\Stack.ps1') @('stop', '-Root', $Root2)
        Check 'restored stack stopped' ($stop2.Code -eq 0 -and (Get-SandboxNodes $Root2).Count -eq 0)
    }
} catch {
    Check "test run: $($_.Exception.Message)" $false
} finally {
    Stop-Sandbox $Root
    Stop-Sandbox $Root2
    $after = (Get-LivePids) -join ' '
    Check "live stack untouched ($after)" ($after -eq $livePids)
    if (-not $KeepSandbox) {
        Copy-Item -LiteralPath $Log -Destination (Join-Path $env:TEMP 'dr-sandbox-test.log') -Force -ErrorAction SilentlyContinue
        Remove-Item -LiteralPath $SandboxDir -Recurse -Force -ErrorAction SilentlyContinue
        Check 'sandbox deleted' (-not (Test-Path -LiteralPath $SandboxDir))
    }
}
Write-Host ''
Write-Host "sandbox test: $script:Pass passed, $script:Fail failed (log: $(if ($KeepSandbox) { $Log } else { Join-Path $env:TEMP 'dr-sandbox-test.log' }))" -ForegroundColor $(if ($script:Fail) { 'Red' } else { 'Green' })
exit $(if ($script:Fail) { 1 } else { 0 })
