<#
.SYNOPSIS
    Unit checks for the Windows Server kit: script parsing, invite strings (v1 and v2), certificate
    fingerprints, TLS pinning, Get-ServerStatus with and without an account key, account key files,
    the chunked-upload helper and the performance sampler. Touches nothing outside -WorkDir and two
    loopback ports (-Port, default 62450, and the one above it).

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-KitUnit.ps1 -WorkDir $env:TEMP\dr-kit-unit
#>
param(
    [string]$WorkDir = (Join-Path $env:TEMP 'dr-kit-unit'),
    [ValidateRange(62000, 62499)][int]$Port = 62450
)
$ErrorActionPreference = 'Stop'
$Kit = Split-Path $PSScriptRoot -Parent
. (Join-Path $Kit 'DauntlessServer.Common.ps1')

$script:Pass = 0; $script:Fail = 0
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
    if ($Ok) { $script:Pass++; Write-Host "  ok    $Name" -ForegroundColor Green }
    else { $script:Fail++; Write-Host "  FAIL  $Name  $Detail" -ForegroundColor Red }
}
function ParseError([string]$Text) { try { [void](ConvertFrom-DRInviteString $Text); return '' } catch { return "$($_.Exception.Message)" } }

if (Test-Path -LiteralPath $WorkDir) { Remove-Item -LiteralPath $WorkDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null

Write-Host '== scripts parse (PowerShell 5.1), ASCII only'
foreach ($f in @(Get-ChildItem -LiteralPath $Kit -Filter *.ps1) + @(Get-ChildItem -LiteralPath $PSScriptRoot -Filter *.ps1)) {
    $tokens = $null; $errors = $null
    [void][Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$tokens, [ref]$errors)
    $nonAscii = @([IO.File]::ReadAllBytes($f.FullName) | Where-Object { $_ -gt 127 }).Count
    Check "$($f.Name) parses" ($errors.Count -eq 0) (($errors | ForEach-Object { "line $($_.Extent.StartLineNumber): $($_.Message)" }) -join '; ')
    Check "$($f.Name) is ASCII" ($nonAscii -eq 0) "$nonAscii bytes > 127"
}

Write-Host '== invite strings'
$pathBefore = $env:Path
$buildEnv = Get-DRBuildEnvironment 'C:\Program Files\nodejs\node.exe'
Check 'build PATH finds the configured Node in an old SSH session' ($buildEnv.Path.StartsWith('C:\Program Files\nodejs;') -and $buildEnv.NODE_ENV -eq 'development')
Check 'build environment does not change the operator PATH' ($env:Path -ceq $pathBefore)
$fp = 'ab' * 32
$v2 = New-DRInviteString -Mode Public -ServerHost '203.0.113.7' -Port 443 -Fingerprint $fp -Code 'ABCD-EFGH-JKLM' -Name "Alex's Ramsgate"
Check 'v2 layout' ($v2 -ceq "dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=$fp&code=ABCD-EFGH-JKLM&name=Alex's%20Ramsgate") $v2
$p = ConvertFrom-DRInviteString $v2
Check 'v2 round trip' ($p.Version -eq 2 -and $p.Mode -eq 'public' -and $p.Host -eq '203.0.113.7' -and $p.Port -eq 443 -and $p.Fingerprint -eq $fp -and $p.Code -eq 'ABCD-EFGH-JKLM' -and $p.Name -eq "Alex's Ramsgate")
$v1 = New-DRInviteString -Mode Private -ServerHost '100.101.102.103' -Port 61000 -Code 'ABCD-EFGH-JKLM' -Name 'Home' -ShareUrl 'https://login.tailscale.com/admin/invite/abc'
$p1 = ConvertFrom-DRInviteString $v1
Check 'v1 round trip' ($p1.Version -eq 1 -and $p1.Mode -eq 'private' -and $p1.Port -eq 61000 -and $p1.Share -eq 'https://login.tailscale.com/admin/invite/abc' -and $null -eq $p1.Fingerprint)
Check 'v2 DNS host' ((ConvertFrom-DRInviteString "dauntless-revived://join?v=2&mode=public&host=play.example.org&port=8443&fp=$fp&code=abcd&name=x").Host -eq 'play.example.org')
Check 'v2 host is lower-cased' ((ConvertFrom-DRInviteString "dauntless-revived://join?v=2&mode=public&host=Play.Example.ORG&port=443&fp=$fp&code=abcd&name=x").Host -eq 'play.example.org')
Check 'join/? form accepted' ((ParseError "dauntless-revived://join/?v=2&mode=public&host=1.2.3.4&port=443&fp=$fp&code=abcd&name=x") -eq '')
$base = "dauntless-revived://join?v=2&mode=public&host=203.0.113.7&port=443&fp=$fp&code=ABCD-EFGH&name=x"
$bad = [ordered]@{
    'v2 without fp'              = @("dauntless-revived://join?v=2&mode=public&host=1.2.3.4&port=443&code=abcd&name=x", 'fp')
    'v2 upper-case fp'           = @(($base -replace "fp=$fp", ('fp=' + $fp.ToUpperInvariant())), 'fp')
    'v2 short fp'                = @(($base -replace "fp=$fp", ('fp=' + $fp.Substring(1))), 'fp')
    'v2 non-hex fp'              = @(($base -replace "fp=$fp", ('fp=' + ('g' * 64))), 'fp')
    'v2 mode=private'            = @(($base -replace 'mode=public', 'mode=private'), 'mode')
    'v2 without mode'            = @(($base -replace 'mode=public&', ''), 'mode')
    'v2 with share'              = @("$base&share=https%3A%2F%2Flogin.tailscale.com%2Fx", 'params')
    'v1 with fp'                 = @("dauntless-revived://join?v=1&host=100.64.0.1&port=61000&fp=$fp&code=abcd&name=x", 'params')
    'v1 with mode'               = @('dauntless-revived://join?v=1&mode=public&host=100.64.0.1&port=61000&code=abcd&name=x', 'params')
    'unknown parameter'          = @("$base&extra=1", 'params')
    'duplicate parameter'        = @("$base&code=WXYZ", 'params')
    'empty pair'                 = @("$base&", 'params')
    'fragment'                   = @("$base#x", 'params')
    'version 3'                  = @(($base -replace 'v=2', 'v=3'), 'version')
    'no version'                 = @(($base -replace 'v=2&', ''), 'version')
    'host with underscore'       = @(($base -replace 'host=203.0.113.7', 'host=my_host'), 'host')
    'host 1.2.3'                 = @(($base -replace 'host=203.0.113.7', 'host=1.2.3'), 'host')
    'host 256.1.1.1'             = @(($base -replace 'host=203.0.113.7', 'host=256.1.1.1'), 'host')
    'port 0'                     = @(($base -replace 'port=443', 'port=0'), 'port')
    'port 65536'                 = @(($base -replace 'port=443', 'port=65536'), 'port')
    'port 0443'                  = @(($base -replace 'port=443', 'port=0443'), 'port')
    'code too short'             = @(($base -replace 'code=ABCD-EFGH', 'code=abc'), 'code')
    'code with space'            = @(($base -replace 'code=ABCD-EFGH', 'code=AB%20CD'), 'code')
    'name with a control char'   = @(($base -replace 'name=x', 'name=a%07b'), 'name')
    'name with a bidi override'  = @(($base -replace 'name=x', 'name=a%E2%80%AEb'), 'name')
    'name empty'                 = @(($base -replace 'name=x', 'name=%20'), 'name')
    'other scheme'               = @(($base -replace '^dauntless-revived', 'https'), 'not_invite')
    'empty'                      = @('   ', 'empty')
}
foreach ($k in $bad.Keys) { $e = ParseError $bad[$k][0]; Check "rejects: $k" ($e -eq $bad[$k][1]) "got '$e'" }
$threw = $false; try { [void](New-DRInviteString -Mode Public -ServerHost '1.2.3.4' -Port 443 -Code 'abcd' -Name 'x') } catch { $threw = $true }
Check 'v2 generator refuses a missing fingerprint' $threw
$threw = $false; try { [void](New-DRInviteString -Mode Public -ServerHost '1.2.3.4' -Port 443 -Fingerprint $fp -Code 'abcd' -Name 'x' -ShareUrl 'https://login.tailscale.com/x') } catch { $threw = $true }
Check 'v2 generator refuses a share link' $threw

Write-Host '== addresses and .env values'
foreach ($a in '203.0.113.7', '10.1.2.3', '100.64.0.1', '127.0.0.1', '172.20.0.1', '192.168.1.1', '169.254.1.1', '224.0.0.1', '0.1.2.3') { Check "not public: $a" (-not (Test-DRPublicIPv4 $a)) }
foreach ($a in '8.8.8.8', '95.216.1.2', '172.32.0.1', '100.128.0.1', '192.0.1.1') { Check "public: $a" (Test-DRPublicIPv4 $a) }
foreach ($a in '1.2.3.4', '1.2.3.0/24', '10.0.0.0/8') { Check "admin ip ok: $a" (Test-DRIPv4Cidr $a) }
foreach ($a in '1.2.3.0/33', '1.2.3.0/4', 'any', '1.2.3', '::1') { Check "admin ip refused: $a" (-not (Test-DRIPv4Cidr $a)) }
$threw = $false; try { [void](Format-DREnvValue 'a"b') } catch { $threw = $true }
Check '.env refuses a quote' $threw
Check '.env quotes spaces' ((Format-DREnvValue "Alex's Ramsgate") -ceq '"Alex''s Ramsgate"')
Check 'PowerShell literal' ((ConvertTo-DRPsLiteral "it's") -ceq "'it''s'")
$envFile = Join-Path $WorkDir 'x.env'
[void](Write-DREnv $envFile ([ordered]@{ A = '1'; SERVER_NAME = 'Two words'; C = 'x=y' }) @('header'))
$back = Read-DREnv $envFile
Check '.env round trip' ($back['A'] -eq '1' -and $back['SERVER_NAME'] -eq 'Two words' -and $back['C'] -eq 'x=y')
Check '.env unchanged write reports no change' (-not (Write-DREnv $envFile ([ordered]@{ A = '1'; SERVER_NAME = 'Two words'; C = 'x=y' }) @('header')))

Write-Host '== certificate fingerprint and TLS pinning'
$gw = Join-Path (Split-Path $Kit -Parent | Split-Path -Parent) 'legacy\UndauntedGateway'
$makeCert = Join-Path $gw 'tools\make-cert.js'
$node = (Get-Command node.exe).Source
$server = $null
if (-not (Test-Path -LiteralPath (Join-Path $gw 'node_modules\node-forge'))) {
    Check 'legacy\UndauntedGateway\node_modules present (npm ci there first)' $false
} else {
    $cert = Join-Path $WorkDir 'cert.pem'; $key = Join-Path $WorkDir 'key.pem'
    $out = & $node $makeCert --host 127.0.0.1 --host test.invalid --cert $cert --key $key --json 2>&1 | Out-String
    $info = $out | ConvertFrom-Json
    $mine = Get-DRCertFingerprint $cert
    Check 'fingerprint matches make-cert' ($mine -eq $info.fingerprint -and (Test-DRFingerprint $mine)) "$mine vs $($info.fingerprint)"
    $ci = Get-DRCertInfo $cert
    Check 'certificate valid about 10 years' ($ci.NotAfter -gt (Get-Date).AddYears(9)) "$($ci.NotAfter)"
    Check 'certificate names' ($ci.San -match '127\.0\.0\.1' -and $ci.San -match 'test\.invalid') $ci.San

    # A stand-in ServerStatus like the metagame's: the player list only for the one account key it
    # knows (a made-up test key), else the limited answer.
    $testKey = 'UUK_' + ('5e7a' * 12)
    $js = Join-Path $WorkDir 'tls-server.js'
    Set-Content -LiteralPath $js -Encoding ASCII -Value @"
const https = require('https'); const fs = require('fs');
const known = process.argv[5];
const s = https.createServer({ cert: fs.readFileSync(process.argv[2]), key: fs.readFileSync(process.argv[3]) }, (req, res) => {
  if (req.url !== '/undaunted/api/ServerStatus') {
    res.writeHead(404, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ name: 'pin test', path: req.url }));
  }
  const full = req.headers['x-undaunted-user-api-key'] === known;
  const at = new Date(Date.now() - 600000).toISOString();
  res.writeHead(200, { 'content-type': 'application/json' });
  res.end(JSON.stringify({ name: 'pin test', online: true, version: '1.0.0', commit: 'abc1234', sourceUrl: 'https://github.com/mixutin/dauntless-revived', registration: 'INVITECODE',
    playersOnline: full ? 2 : 0,
    players: full ? [{ name: 'Aurora', where: 'city', instance: 'c1' }, { name: 'Borealis', where: 'hunt', instance: 'h1' }] : [],
    instances: full ? [{ id: 'c1', kind: 'city', title: 'Ramsgate', map: 'ramsgate_01', behemoth: null, players: 1, maxPlayers: 32, startedAt: at },
                       { id: 'h1', kind: 'hunt', title: 'Hunt: Shrike', map: 'island', behemoth: 'Shrike', players: 1, maxPlayers: 4, startedAt: at }] : [],
    contentPort: null, uptimeSeconds: 60, limited: !full }));
});
s.listen(Number(process.argv[4]), '127.0.0.1', () => console.log('listening'));
"@
    $psi = New-Object Diagnostics.ProcessStartInfo($node, ('"{0}" "{1}" "{2}" {3} {4}' -f $js, $cert, $key, $Port, $testKey))
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true; $psi.RedirectStandardOutput = $true
    $server = [Diagnostics.Process]::Start($psi)
    try {
        [void]$server.StandardOutput.ReadLine()
        $ok = Invoke-DRHttp -Url "https://127.0.0.1:$Port/undaunted/api/ServerStatus" -Fingerprint $mine -TimeoutSec 5
        Check 'pinned TLS with the right fingerprint' ($ok.Status -eq 200 -and $ok.Json.name -eq 'pin test' -and $ok.SeenFingerprint -eq $mine) "$($ok.Status) $($ok.Error)"
        $wrong = ('0' * 63) + '1'
        $no = Invoke-DRHttp -Url "https://127.0.0.1:$Port/undaunted/api/ServerStatus" -Fingerprint $wrong -TimeoutSec 5
        Check 'pinned TLS refuses another certificate' ($no.Status -eq 0 -and $no.Error -match 'fingerprint mismatch' -and $no.SeenFingerprint -eq $mine) "$($no.Status) $($no.Error)"
        $none = Invoke-DRHttp -Url "https://127.0.0.1:$Port/undaunted/api/ServerStatus" -TimeoutSec 5
        Check 'https without a fingerprint is refused before connecting' ($none.Status -eq 0 -and $none.Error -match 'fingerprint')
        $gs = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit 'Get-ServerStatus.ps1') -Server "127.0.0.1:$Port" -Fingerprint $wrong 2>&1 | Out-String
        Check 'Get-ServerStatus warns about a different certificate' ($LASTEXITCODE -eq 1 -and $gs -match 'DIFFERENT certificate') $gs

        # The player list is for registered players only: with the key it shows, without it the
        # script says it is hidden (never "0 players"), and the key is never printed.
        function GetStatus([string[]]$More) {
            $o = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit 'Get-ServerStatus.ps1') -Server "127.0.0.1:$Port" -Fingerprint $mine @More 2>&1 | Out-String
            return [pscustomobject]@{ Code = $LASTEXITCODE; Text = $o }
        }
        $keyFile = Join-Path $WorkDir 'account.key'
        [IO.File]::WriteAllText($keyFile, "$testKey`r`n")
        $backupFile = Join-Path $WorkDir 'key-backup.txt'
        [IO.File]::WriteAllText($backupFile, "Dauntless Revived account key`r`n`r`nServer: pin test (127.0.0.1:$Port)`r`nUsername: Aurora`r`nKey: $testKey`r`n`r`nKeep this file private.`r`n")
        $wrongFile = Join-Path $WorkDir 'wrong.key'
        [IO.File]::WriteAllText($wrongFile, 'UUK_' + ('0' * 48))
        $none = GetStatus @()
        Check 'no key: the list is hidden, not "0 players"' ($none.Code -eq 0 -and $none.Text -match 'Player list hidden: this server shows who is online' -and $none.Text -match 'pass -KeyFile' -and $none.Text -notmatch 'Players online' -and $none.Text -notmatch 'Worlds and hunts running') $none.Text
        $with = GetStatus @('-KeyFile', $keyFile)
        Check 'with -KeyFile: players and worlds listed' ($with.Code -eq 0 -and $with.Text -match 'Players online: 2' -and $with.Text -match 'Aurora\s+in the city - Ramsgate' -and $with.Text -match 'Worlds and hunts running: 2' -and $with.Text -notmatch 'hidden') $with.Text
        Check 'with -KeyFile: the key is not printed' (-not $with.Text.Contains($testKey) -and -not $none.Text.Contains($testKey))
        $bk = GetStatus @('-KeyFile', $backupFile)
        Check '-KeyFile takes the launcher''s key backup file' ($bk.Code -eq 0 -and $bk.Text -match 'Players online: 2') $bk.Text
        $wr = GetStatus @('-KeyFile', $wrongFile)
        Check 'a key the server does not accept: hidden, and says so' ($wr.Code -eq 0 -and $wr.Text -match 'Player list hidden' -and $wr.Text -match 'did not accept the key in') $wr.Text
        $js0 = GetStatus @('-Json')
        $j = $null; try { $j = $js0.Text | ConvertFrom-Json } catch {}
        Check '-Json passes the limited answer through unchanged' ($j -and $j.limited -eq $true -and $j.playersOnline -eq 0 -and $j.name -eq 'pin test') $js0.Text
        $plain = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File (Join-Path $Kit 'Get-ServerStatus.ps1') -Server '203.0.113.7' -KeyFile $keyFile -TimeoutSec 1 2>&1 | Out-String
        Check 'no key over plain HTTP to a public address' ($LASTEXITCODE -eq 1 -and $plain -match 'goes only over TLS pinned') $plain
    } finally { if ($server -and -not $server.HasExited) { $server.Kill(); $server.WaitForExit(5000) | Out-Null } }
}

Write-Host '== account keys'
$kd = Join-Path $WorkDir 'keys'
New-Item -ItemType Directory -Force -Path $kd | Out-Null
[IO.File]::WriteAllText((Join-Path $kd 'plain.key'), "  UUK_abcdef0123456789  `r`n")
[IO.File]::WriteAllText((Join-Path $kd 'junk.txt'), "not a key at all`r`nsecond line")
Check 'key file: the key alone, trimmed' ((Read-DRAccountKey (Join-Path $kd 'plain.key')) -ceq 'UUK_abcdef0123456789')
Check 'key file: no key in it -> $null' ($null -eq (Read-DRAccountKey (Join-Path $kd 'junk.txt')))
foreach ($h in '127.0.0.1', 'localhost', '100.64.0.1', '100.127.255.254', 'box.tail1234.ts.net') { Check "key over plain HTTP allowed: $h" (Test-DRPlainKeyHost $h) }
foreach ($h in '203.0.113.7', '100.128.0.1', '10.0.0.5', 'ts.net', 'example.org', 'evil.ts.net.example.org') { Check "key over plain HTTP refused: $h" (-not (Test-DRPlainKeyHost $h)) }

Write-Host '== chunked upload helper (Receive-Upload.ps1)'
$up = Join-Path $WorkDir 'upload'
New-Item -ItemType Directory -Force -Path $up | Out-Null
$data = New-Object byte[] (2.5MB); (New-Object Random 7).NextBytes($data)
$chunk = 1MB
$segs = @(); $hashes = @(); for ($i = 0; $i -lt 3; $i++) { $len = [math]::Min($chunk, $data.Length - $i * $chunk); $seg = New-Object byte[] $len; [Array]::Copy($data, $i * $chunk, $seg, 0, $len); $segs += ,$seg; $sha = [Security.Cryptography.SHA256]::Create(); $hashes += (ConvertTo-DRHex $sha.ComputeHash($seg)) }
$sha = [Security.Cryptography.SHA256]::Create(); $whole = ConvertTo-DRHex $sha.ComputeHash($data)
Write-DRText -Path (Join-Path $up 'upload.json.incoming') -Text (@{ name = 'test.zip'; size = $data.Length; sha256 = $whole; chunkSize = $chunk; chunks = $hashes } | ConvertTo-Json -Compress)
function Recv([string]$Action, [int]$Index = -1) {
    $a = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', (Join-Path $Kit 'Receive-Upload.ps1'), '-Action', $Action, '-Dir', $up)
    if ($Index -ge 0) { $a += @('-Index', "$Index") }
    $l = & powershell.exe @a 2>&1 | Where-Object { "$_" -match '^DRJSON:' } | Select-Object -Last 1
    return ("$l".Substring(7) | ConvertFrom-Json)
}
$b = Recv 'Begin'
Check 'Begin: nothing verified yet' (-not $b.complete -and @($b.verified).Count -eq 0 -and $b.count -eq 3)
for ($i = 0; $i -lt 3; $i++) { [IO.File]::WriteAllBytes((Join-Path $up ('part{0:D5}' -f $i)), $segs[$i]) }
Check 'Verify part 1' ((Recv 'Verify' 0).ok -and (Recv 'Verify' 1).ok)
$f = [IO.File]::Open((Join-Path $up 'part00002'), 'Open', 'ReadWrite'); $f.WriteByte(1); $f.Dispose()
$v = Recv 'Verify' 2
Check 'Verify refuses a damaged part and deletes it' (-not $v.ok -and $v.reason -eq 'hash' -and -not (Test-Path -LiteralPath (Join-Path $up 'part00002')))
$a = Recv 'Assemble'
Check 'Assemble reports the missing part' (-not $a.complete -and @($a.missing) -contains 2)
$seg = New-Object byte[] ($data.Length - 2 * $chunk); [Array]::Copy($data, 2 * $chunk, $seg, 0, $seg.Length); [IO.File]::WriteAllBytes((Join-Path $up 'part00002'), $seg)
[void](Recv 'Verify' 2)
$a = Recv 'Assemble'
Check 'Assemble completes with the right SHA-256' ($a.complete -and (Get-DRSha256 (Join-Path $up 'test.zip')).ToLowerInvariant() -eq $whole) ($a | ConvertTo-Json -Compress)
Write-DRText -Path (Join-Path $up 'upload.json.incoming') -Text (@{ name = 'test.zip'; size = $data.Length; sha256 = $whole; chunkSize = $chunk; chunks = $hashes } | ConvertTo-Json -Compress)
Check 'Begin after completion reports complete' ((Recv 'Begin').complete)

Write-Host '== performance sampler (DauntlessServer.Performance.ps1)'
. (Join-Path $Kit 'DauntlessServer.Performance.ps1')
Check 'CPU: 15 s of CPU in 60 s is 25% of one core' ((Get-DRCpuCorePercent 10 25 60) -eq 25)
Check 'CPU: two busy cores read 200%' ((Get-DRCpuCorePercent 0 120 60) -eq 200)
Check 'CPU: no time passed -> empty' ($null -eq (Get-DRCpuCorePercent 10 25 0))
Check 'CPU: a counter that went backwards -> empty' ($null -eq (Get-DRCpuCorePercent 25 10 60))
Check 'host CPU from the idle counter' ((Get-DRHostCpuPercent ([pscustomobject]@{ Idle = 1000; Time = 2000 }) ([pscustomobject]@{ Idle = 1500; Time = 3000 })) -eq 50)
Check 'host CPU without an earlier reading -> empty' ($null -eq (Get-DRHostCpuPercent $null ([pscustomobject]@{ Idle = 1; Time = 2 })))
Check 'network: 750 kB in 60 s is 100 kbit/s' ((Get-DRKbitPerSecond 0 750000 60) -eq 100)
foreach ($r in @(@(8777, 8777, '', 'ramsgate'), @(8776, 8777, '', 'dojo'), @(8770, 8777, '', 'hunt'), @(8769, 8769, '', 'ramsgate'),
                 @(8768, 8769, '', 'dojo'), @(8777, 8769, '', 'hunt'), @(8775, 8777, 'tutorial', 'tutorial'), @(8770, 8777, 'city', 'ramsgate'), @(0, 8777, '', 'unknown'))) {
    Check "role: UDP $($r[0]) with UdpPortEnd $($r[1]) and kind '$($r[2])' is $($r[3])" ((Get-DRGameServerRole $r[0] $r[1] $r[2]) -ceq $r[3])
}
Check 'fixed CSV header' ((Get-DRPerfHeader) -ceq 'timestamp_utc,role,pid,udp_port,started_utc,players,cpu_core_percent,working_set_mb,private_mb,host_cpu_percent,logical_cpus,ram_total_mb,ram_free_mb,disk_free_gb,net_in_kbit_s,net_out_kbit_s,game_servers,players_online')
Check 'one file per UTC day' ((Get-DRPerfFileName (New-Object DateTime 2026, 9, 22, 23, 59, 59, ([DateTimeKind]::Utc))) -ceq 'performance-2026-09-22.csv')
$fiLine = & {
    [Threading.Thread]::CurrentThread.CurrentCulture = New-Object Globalization.CultureInfo 'fi-FI'
    $row = New-DRPerfRow (New-Object DateTime 2026, 9, 22, 12, 0, 0, ([DateTimeKind]::Utc)) 'hunt'
    $row.cpu_core_percent = 12.5; $row.working_set_mb = 1100.25
    @((1.5).ToString(), (ConvertTo-DRPerfCsvLine $row))
}
Check 'a decimal point, also on a Finnish Windows' ($fiLine[0] -eq '1,5' -and $fiLine[1] -ceq ('2026-09-22T12:00:00Z,hunt,,,,,12.5,1100.25' + (',' * 10))) "$($fiLine -join ' | ')"
$deployList = @(
    [pscustomobject]@{ id = 'c1'; port = 8777; kind = 'city'; expectedPlayers = @(); startedAt = '2026-09-22T10:00:00.000Z' },
    [pscustomobject]@{ id = 'h0'; port = 8775; kind = 'hunt'; expectedPlayers = @('acct-old'); startedAt = '2026-09-22T10:05:00.000Z' },
    [pscustomobject]@{ id = 'h1'; port = 8775; kind = 'tutorial'; expectedPlayers = @('acct-new'); startedAt = '2026-09-22T10:30:00.000Z' },
    [pscustomobject]@{ id = 'd1'; port = 8776; kind = 'dojo'; expectedPlayers = @(); startedAt = '2026-09-22T10:10:00.000Z' })
$inst = @([pscustomobject]@{ id = 'c1'; players = 2 }, [pscustomobject]@{ id = 'h0'; players = 9 }, [pscustomobject]@{ id = 'h1'; players = 1 })
$bp = Get-DRGameServerCounts $deployList $inst
Check 'players per game server: the deploy list joined to ServerStatus by id' ($bp[8777].Kind -eq 'city' -and $bp[8777].Players -eq 2 -and $bp[8776].Kind -eq 'dojo' -and $null -eq $bp[8776].Players)
Check 'a reused port counts its newest server' ($bp[8775].Kind -eq 'tutorial' -and $bp[8775].Players -eq 1)

# A stand-in install: server.json, an owner key, and one local HTTP server that answers both the deploy
# server's /gameservers and the metagame's ServerStatus with names, account ids and the key it expects.
$perfPort = if ($Port -lt 62499) { $Port + 1 } else { $Port - 1 }
$perfRoot = Join-Path $WorkDir 'perf-root'
$pp = Get-DRPaths $perfRoot
New-Item -ItemType Directory -Force -Path $pp.Config, $pp.Keys, $pp.Logs | Out-Null
Write-DRText -Path $pp.ServerJson -Text (@{ Version = 2; Mode = 'Public'; BindAddress = '127.0.0.1'; Ports = @{ metagame = $perfPort; deploy = $perfPort }; Components = @('metagame', 'deploy'); GameDir = ''; UdpPortEnd = 8777 } | ConvertTo-Json -Depth 4)
$perfKey = 'UUK_' + ('0f1e' * 12)
[IO.File]::WriteAllText($pp.OwnerKey, $perfKey)
$perfJs = Join-Path $WorkDir 'perf-server.js'
Set-Content -LiteralPath $perfJs -Encoding ASCII -Value @"
const http = require('http');
const key = process.argv[3];
const at = new Date(Date.now() - 600000).toISOString();
http.createServer((req, res) => {
  res.writeHead(200, { 'content-type': 'application/json' });
  if (req.url === '/gameservers') {
    return res.end(JSON.stringify({ servers: [{ id: 'c1', port: 8777, kind: 'city', map: 'ramsgate', gameMode: null, behemoth: null, huntId: null, matchmakerHuntId: null, expectedPlayers: ['acct-secret-1111:x'], maxPlayers: null, startedAt: at }] }));
  }
  const full = req.headers['x-undaunted-user-api-key'] === key;
  res.end(JSON.stringify({ name: 'perf test', online: true, registration: 'INVITECODE', playersOnline: full ? 2 : 0,
    players: full ? [{ name: 'Aurora', where: 'city', instance: 'c1' }, { name: 'Borealis', where: 'menu', instance: null }] : [],
    instances: full ? [{ id: 'c1', kind: 'city', title: 'Ramsgate', map: 'ramsgate', behemoth: null, players: 1, maxPlayers: 32, startedAt: at }] : [],
    contentPort: null, uptimeSeconds: 60, limited: !full }));
}).listen(Number(process.argv[2]), '127.0.0.1', () => console.log('listening'));
"@
$psi = New-Object Diagnostics.ProcessStartInfo($node, ('"{0}" {1} {2}' -f $perfJs, $perfPort, $perfKey))
$psi.UseShellExecute = $false; $psi.CreateNoWindow = $true; $psi.RedirectStandardOutput = $true
$perfServer = [Diagnostics.Process]::Start($psi)
try {
    [void]$perfServer.StandardOutput.ReadLine()
    $cfgPerf = Get-DRConfig $perfRoot
    $st = New-DRPerfState
    $first = @(Invoke-DRPerfSample -State $st -Paths $pp -Config $cfgPerf)
    Start-Sleep -Milliseconds 1200
    $second = @(Invoke-DRPerfSample -State $st -Paths $pp -Config $cfgPerf)
    $h1 = $first[0]; $h2 = $second[0]
    Check 'a host row first, with the machine''s numbers' ($h1.role -eq 'host' -and $h1.logical_cpus -gt 0 -and $h1.ram_total_mb -gt 0 -and $h1.ram_free_mb -gt 0 -and $h1.disk_free_gb -gt 0 -and $h1.game_servers -eq 0) (($h1.GetEnumerator() | ForEach-Object { "$($_.Key)=$($_.Value)" }) -join ' ')
    Check 'players online from ServerStatus, asked with the owner key' ($h1.players_online -eq 2)
    Check 'the first sample has no CPU yet, the second has' ($null -eq $h1.host_cpu_percent -and $null -ne $h2.host_cpu_percent -and $h2.host_cpu_percent -ge 0 -and $h2.host_cpu_percent -le 100) "$($h2.host_cpu_percent)"
    Check 'no processes of this stand-in install' ($first.Count -eq 1 -and $second.Count -eq 1)
    $perfDir = Get-DRPerfDir $pp
    $today = [datetime]::UtcNow
    $file = Write-DRPerfRows $perfDir $today $first
    [void](Write-DRPerfRows $perfDir $today $second)
    $lines = @(Get-Content -LiteralPath $file)
    Check 'the day file: one header, then one line per row' ($file -like '*\data\logs\performance\performance-*.csv' -and $lines.Count -eq 3 -and $lines[0] -ceq (Get-DRPerfHeader) -and $lines[1] -match '^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ,host,') ($lines -join ' | ')
    $text = [IO.File]::ReadAllText($file)
    Check 'the log holds no names, account ids or key' (-not ($text -match 'Aurora|Borealis|acct-secret|perf test') -and -not $text.Contains($perfKey))
    Move-Item -LiteralPath $pp.OwnerKey -Destination "$($pp.OwnerKey).away"
    $noKey = @(Invoke-DRPerfSample -State (New-DRPerfState) -Paths $pp -Config $cfgPerf)
    Move-Item -LiteralPath "$($pp.OwnerKey).away" -Destination $pp.OwnerKey
    Check 'without the owner key the count stays empty, not 0' ($null -eq $noKey[0].players_online)
    $gone = @(Invoke-DRPerfSample -State (New-DRPerfState) -Paths $pp -Config ([pscustomobject]@{ BindAddress = '127.0.0.1'; Ports = [pscustomobject]@{ metagame = 1; deploy = 1 }; Components = @('metagame', 'deploy') }))
    Check 'a metagame that does not answer leaves the count empty' ($gone[0].role -eq 'host' -and $null -eq $gone[0].players_online)
} finally { if ($perfServer -and -not $perfServer.HasExited) { $perfServer.Kill(); $perfServer.WaitForExit(5000) | Out-Null } }

# Planted links in the service-writable logs folder: never written through.
$elsewhere = Join-Path $WorkDir 'perf-elsewhere'
New-Item -ItemType Directory -Force -Path $elsewhere | Out-Null
$jroot = Join-Path $WorkDir 'perf-junction'
$jp = Get-DRPaths $jroot
New-Item -ItemType Directory -Force -Path $jp.Logs | Out-Null
$jperf = Get-DRPerfDir $jp
[void](& cmd.exe /d /c mklink /J "$jperf" "$elsewhere")
$threw = $false; try { [void](Write-DRPerfRows $jperf ([datetime]::UtcNow) $first) } catch { $threw = "$_" -match 'junction or symbolic link' }
Check 'refuses to write when the performance folder is a junction' ($threw -and @(Get-ChildItem -LiteralPath $elsewhere -Force).Count -eq 0)
$tickState = New-DRPerfState
$m1 = Invoke-DRPerfTick -State $tickState -Paths $jp -Config ([pscustomobject]@{ Components = @() })
$m2 = Invoke-DRPerfTick -State $tickState -Paths $jp -Config ([pscustomobject]@{ Components = @() })
Check 'the supervisor hears about a skipped sample once, not every minute' ($m1 -match 'junction' -and $null -eq $m2 -and @(Get-ChildItem -LiteralPath $elsewhere -Force).Count -eq 0) "$m1 / $m2"
[IO.Directory]::Delete($jperf, $false)
New-Item -ItemType Directory -Force -Path $jperf | Out-Null
$outside = Join-Path $WorkDir 'perf-outside.txt'
[IO.File]::WriteAllText($outside, 'untouched')
[void](& cmd.exe /d /c mklink /H "$(Join-Path $jperf (Get-DRPerfFileName ([datetime]::UtcNow)))" "$outside")
$threw = $false; try { [void](Write-DRPerfRows $jperf ([datetime]::UtcNow) $first) } catch { $threw = "$_" -match 'hard link' }
Check 'refuses to write through a hard link' ($threw -and [IO.File]::ReadAllText($outside) -ceq 'untouched')

$prune = Join-Path $WorkDir 'perf-prune'
New-Item -ItemType Directory -Force -Path $prune | Out-Null
foreach ($n in 'performance-2026-08-22.csv', 'performance-2026-08-23.csv', 'performance-2026-08-24.csv', 'performance-2026-09-22.csv', 'performance-latest.csv', 'notes.txt') { [IO.File]::WriteAllText((Join-Path $prune $n), 'x') }
$removed = Remove-DROldPerfFiles $prune (New-Object DateTime 2026, 9, 22, 8, 0, 0, ([DateTimeKind]::Utc)) 30
$left = @(Get-ChildItem -LiteralPath $prune | ForEach-Object { $_.Name } | Sort-Object)
Check 'keeps 30 days of files and nothing else is touched' ($removed -eq 2 -and ($left -join ',') -ceq 'notes.txt,performance-2026-08-24.csv,performance-2026-09-22.csv,performance-latest.csv') "$removed removed; left $($left -join ',')"

Write-Host '== chat settings (installer -Chat, Set-Chat.ps1, status line)'
foreach ($sb in $false, $true) {
    $m = [ordered]@{}
    Set-DRChatEnv $m 'On' $sb
    $wsPort = ([Uri](Get-DRGatewayWsUrl $sb)).Port
    Check "CHAT_PORT is the gateway's WebSocket port ($(if ($sb) { 'sandbox' } else { 'normal' }))" ($m['CHAT_PORT'] -eq "$wsPort" -and $m['CHAT'] -eq '1' -and $m['CHAT_BIND_HOST'] -eq '127.0.0.1') "$($m['CHAT_PORT']) vs $wsPort"
}
Check 'normal install: 61099, sandbox: 62099' ((Get-DRChatPort $false) -eq 61099 -and (Get-DRChatPort $true) -eq 62099)
$m = [ordered]@{}; Set-DRChatEnv $m 'Off' $false
Check 'Off writes CHAT=0 and keeps the address and port' ($m['CHAT'] -eq '0' -and $m['CHAT_BIND_HOST'] -eq '127.0.0.1' -and $m['CHAT_PORT'] -eq '61099')
Check '-Chat wins over server.json' ((Resolve-DRChat 'On' ([pscustomobject]@{ Chat = 'Off' })) -eq 'On' -and (Resolve-DRChat 'Off' ([pscustomobject]@{ Chat = 'On' })) -eq 'Off')
Check 'without -Chat, server.json''s "Chat" is kept' ((Resolve-DRChat '' ([pscustomobject]@{ Chat = 'On' })) -eq 'On')
Check 'a new public install defaults On' ((Resolve-DRChat '' $null) -eq 'On' -and (Resolve-DRChat '' ([pscustomobject]@{ Mode = 'Public' })) -eq 'On')
Check 'an explicit saved Off remains Off' ((Resolve-DRChat '' ([pscustomobject]@{ Chat = 'Off' })) -eq 'Off')
$chatEnv = Join-Path $WorkDir 'chat-metagame.env'
$first = [ordered]@{ PORT = '61000'; PROGRESSION_MODE = 'real' }
Set-DRChatEnv $first 'On' $false
[void](Write-DREnv $chatEnv $first @('test'))
$rerun = Read-DREnv $chatEnv
Set-DRChatEnv $rerun (Resolve-DRChat '' ([pscustomobject]@{ Chat = 'On' })) $false
Check 'a re-run keeps the chat keys and every other key (nothing to write)' (-not (Write-DREnv $chatEnv $rerun @('test')) -and (Read-DREnv $chatEnv)['PROGRESSION_MODE'] -eq 'real')
$chatPaths = [pscustomobject]@{ MetaEnv = $chatEnv }
$listener = New-Object Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 0)
$listener.Start()
try {
    $freePort = $listener.LocalEndpoint.Port
    [void](Write-DREnv $chatEnv ([ordered]@{ CHAT = '1'; CHAT_BIND_HOST = '127.0.0.1'; CHAT_PORT = "$freePort" }) @('test'))
    Check 'status: listening' ((Get-DRChatState $chatPaths $null) -eq "listening 127.0.0.1:$freePort") (Get-DRChatState $chatPaths $null)
    [void](Write-DREnv $chatEnv ([ordered]@{ CHAT_PORT = "$freePort" }) @('test'))
    Check 'status: unset CHAT is on by default' ((Get-DRChatState $chatPaths $null) -eq "listening 127.0.0.1:$freePort")
} finally { $listener.Stop() }
Check 'status: on but not listening' ((Get-DRChatState $chatPaths $null) -eq 'on in metagame.env but not listening (see the metagame log)') (Get-DRChatState $chatPaths $null)
[void](Write-DREnv $chatEnv ([ordered]@{ CHAT = '0'; CHAT_PORT = '61099' }) @('test'))
Check 'status: off' ((Get-DRChatState $chatPaths $null) -like 'off (CHAT setting*')
Check 'status: unknown without a metagame.env' ((Get-DRChatState ([pscustomobject]@{ MetaEnv = (Join-Path $WorkDir 'none.env') }) $null) -eq 'unknown (metagame.env missing)')

# Set-Chat.ps1 on an install whose metagame.env the installer or an update wrote (their header, other keys
# changed since): the setting is already as asked, so nothing is written and the stack is not touched
$setChatRoot = Join-Path $WorkDir 'setchat-root'
$setChatPaths = Get-DRPaths $setChatRoot
New-Item -ItemType Directory -Force -Path $setChatPaths.Config, $setChatPaths.Bin | Out-Null
$stackMarker = Join-Path $WorkDir 'setchat-stack-called.txt'
[IO.File]::WriteAllText((Join-Path $setChatPaths.Bin 'Stack.ps1'), "param([string]`$Action, [string]`$Root)`r`nAdd-Content -LiteralPath '$stackMarker' -Value `$Action`r`n")
[IO.File]::WriteAllText($setChatPaths.ServerJson, '{ "Mode": "Public", "Sandbox": true, "Chat": "On" }')
$installed = [ordered]@{ PORT = '62000'; GIT_COMMIT = 'abc1234' }
Set-DRChatEnv $installed 'On' $true
[void](Write-DREnv $setChatPaths.MetaEnv $installed $script:DRSecretEnvHeader)
$installedText = [IO.File]::ReadAllText($setChatPaths.MetaEnv)
$setChatOut = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $Kit 'Set-Chat.ps1') -Root $setChatRoot -On 2>&1 | Out-String
Check 'Set-Chat -On after an install or update: "already on", the stack untouched' ($LASTEXITCODE -eq 0 -and $setChatOut -match 'chat is already on' -and -not (Test-Path -LiteralPath $stackMarker)) $setChatOut.Trim()
Check 'Set-Chat -On after an install or update: metagame.env unchanged' ([IO.File]::ReadAllText($setChatPaths.MetaEnv) -ceq $installedText)
[IO.File]::WriteAllText($setChatPaths.MetaEnv, "# an older header`r`n" + ($installedText -replace '(?m)^#.*\r?\n', ''))
$setChatOut = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $Kit 'Set-Chat.ps1') -Root $setChatRoot -On 2>&1 | Out-String
Check 'Set-Chat -On with any other header: still "already on"' ($LASTEXITCODE -eq 0 -and $setChatOut -match 'chat is already on' -and -not (Test-Path -LiteralPath $stackMarker)) $setChatOut.Trim()
Check 'every script writes the same header for metagame.env' (@(Select-String -LiteralPath (Join-Path $Kit 'Install-DauntlessServer.ps1'), (Join-Path $Kit 'Update-DauntlessServer.ps1'), (Join-Path $Kit 'Set-Chat.ps1') -Pattern 'Holds secrets|holds secrets').Count -eq 0)
# A real change: written with the shared header, the stack restarted once (the stub records it), the
# listener found stopped (nothing listens on the sandbox chat port here)
if (@(Get-NetTCPConnection -State Listen -LocalPort (Get-DRChatPort $true) -ErrorAction SilentlyContinue).Count -eq 0) {
    $setChatOut = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $Kit 'Set-Chat.ps1') -Root $setChatRoot -Off 2>&1 | Out-String
    $offEnv = Read-DREnv $setChatPaths.MetaEnv
    Check 'Set-Chat -Off: "chat off" after one stop and start' ($LASTEXITCODE -eq 0 -and $setChatOut -match 'chat off' -and (Test-Path -LiteralPath $stackMarker) -and ((Get-Content -LiteralPath $stackMarker) -join ',') -eq 'stop,start') $setChatOut.Trim()
    Check 'Set-Chat -Off: CHAT=0 under the shared header, other keys kept, "Chat": "Off"' ($offEnv['CHAT'] -eq '0' -and $offEnv['GIT_COMMIT'] -eq 'abc1234' -and ([IO.File]::ReadAllLines($setChatPaths.MetaEnv)[0] -ceq "# $($script:DRSecretEnvHeader[0])") -and (Get-Content -LiteralPath $setChatPaths.ServerJson -Raw | ConvertFrom-Json).Chat -eq 'Off')
} else { Write-Host '  skip  Set-Chat -Off (something listens on the sandbox chat port)' -ForegroundColor DarkGray }

Write-Host ''
Write-Host "unit checks: $script:Pass passed, $script:Fail failed" -ForegroundColor $(if ($script:Fail) { 'Red' } else { 'Green' })
Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
exit $(if ($script:Fail) { 1 } else { 0 })
