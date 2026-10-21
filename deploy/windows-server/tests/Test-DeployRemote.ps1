<#
.SYNOPSIS
    Tests Deploy-Remote.ps1 without a server: argument handling (nothing connects), and the kit, source,
    backup and chunked game-zip uploads against a local folder that stands in for the server
    (-TestTargetDir): an interrupted upload that resumes, a part damaged in transit, a part damaged after
    it was verified, and a repeated run that has nothing left to do. Uses no network and no SSH key.

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File deploy\windows-server\tests\Test-DeployRemote.ps1
#>
param([string]$WorkDir = (Join-Path $env:TEMP 'dr-deploy-test'))
$ErrorActionPreference = 'Stop'
$Kit = Split-Path $PSScriptRoot -Parent
. (Join-Path $Kit 'DauntlessServer.Common.ps1')
$Deploy = Join-Path $Kit 'Deploy-Remote.ps1'

$script:Pass = 0; $script:Fail = 0
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
    if ($Ok) { $script:Pass++; Write-Host "  ok    $Name" -ForegroundColor Green }
    else { $script:Fail++; Write-Host "  FAIL  $Name  $Detail" -ForegroundColor Red }
}
function Run([string[]]$Arguments) {
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $out = & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $Deploy @Arguments 2>&1 | ForEach-Object { "$_" }; $code = $LASTEXITCODE }
    finally { $ErrorActionPreference = $old }
    return [pscustomobject]@{ Code = $code; Out = $out; Text = ($out -join "`n") }
}
function New-RandomFile([string]$Path, [long]$Size, [int]$Seed) {
    $rng = New-Object Random $Seed
    $fs = [IO.File]::Create($Path)
    try { $buf = New-Object byte[] (1MB); $left = $Size; while ($left -gt 0) { $rng.NextBytes($buf); $n = [int][math]::Min($buf.Length, $left); $fs.Write($buf, 0, $n); $left -= $n } } finally { $fs.Dispose() }
}

if (Test-Path -LiteralPath $WorkDir) { Remove-Item -LiteralPath $WorkDir -Recurse -Force }
New-Item -ItemType Directory -Force -Path $WorkDir | Out-Null
$remote = Join-Path $WorkDir 'server'
$zip = Join-Path $WorkDir 'BaseGame144.zip'
New-RandomFile $zip (36MB + 12345) 1
$zipHash = (Get-DRSha256 $zip).ToLowerInvariant()
$backup = Join-Path $WorkDir '2026-10-01_200000'
New-Item -ItemType Directory -Force -Path (Join-Path $backup 'secrets') | Out-Null
New-RandomFile (Join-Path $backup 'undaunted.db') 200000 2
Set-Content -LiteralPath (Join-Path $backup 'secrets\metagame.env') -Value 'TEST_ONLY=1' -Encoding ASCII
Set-Content -LiteralPath (Join-Path $backup 'secrets\deployserver.env') -Value 'TEST_ONLY=1' -Encoding ASCII
$dummyKey = Join-Path $WorkDir 'not_a_key'
Set-Content -LiteralPath $dummyKey -Value 'placeholder' -Encoding ASCII
$t = @('-Server', 'test.invalid', '-TestTargetDir', $remote)

try {
    Write-Host '== argument handling (nothing connects)'
    $cases = [ordered]@{
        'bad server name'                         = @(@('-Server', 'bad host!', '-OwnerName', 'Tester'), 'must be an IPv4 address or a DNS name')
        'test switch without a test folder'       = @(@('-Server', 'test.invalid', '-TestZipSha256', $zipHash, '-OwnerName', 'Tester'), 'only works with -TestTargetDir')
        'zip and zip URL together'                = @(($t + @('-GameZip', $zip, '-GameZipUrl', 'https://example.org/x.zip', '-OwnerName', 'Tester')), 'not both')
        'zip URL without https'                   = @(($t + @('-GameZipUrl', 'http://example.org/x.zip', '-OwnerName', 'Tester')), 'https://')
        'install without owner or backup'         = @($t, 'needs -OwnerName')
        'bad owner name'                          = @(($t + @('-OwnerName', 'no spaces allowed')), '3-16 letters')
        'backup without deployserver.env'         = @(($t + @('-RestoreFrom', (Join-Path $backup 'secrets'))), 'is missing')
        'update with a game zip'                  = @(($t + @('-Update', '-GameZip', $zip)), 'belong to an install')
        'two actions'                             = @(($t + @('-Update', '-Status')), 'Use one of')
        'install root with a space'               = @(($t + @('-OwnerName', 'Tester', '-InstallRoot', 'C:\Dauntless Revived')), 'without spaces')
        'admin ip "any"'                          = @(($t + @('-OwnerName', 'Tester', '-AdminIp', 'any')), 'IPv4 address or IPv4/prefix')
        'working tree with GitHub source'         = @(($t + @('-OwnerName', 'Tester', '-Source', 'GitHub', '-WorkingTree')), 'does not go with')
        'missing key file'                        = @(@('-Server', 'test.invalid', '-OwnerName', 'Tester', '-KeyFile', (Join-Path $WorkDir 'nope')), 'SSH key not found')
        'invite name with a quote'                = @(($t + @('-InviteFor', "a'b")), 'plain text')
        'chat with an update'                     = @(($t + @('-Update', '-Chat', 'On')), '-Chat goes with an install')
        'chat on for a private install'           = @(($t + @('-OwnerName', 'Tester', '-Mode', 'Private', '-Chat', 'On')), 'public mode only')
    }
    foreach ($k in $cases.Keys) {
        $r = Run $cases[$k][0]
        Check "refuses: $k" ($r.Code -eq 1 -and $r.Text -match [regex]::Escape($cases[$k][1])) (($r.Out | Where-Object { $_ -match 'FAIL' }) -join ' | ')
    }
    $w = Run @('-Server', '203.0.113.7', '-KeyFile', $dummyKey, '-GameZip', $zip, '-RestoreFrom', $backup, '-AdminIp', '198.51.100.20', '-WhatIf')
    Check '-WhatIf prints the plan without connecting' ($w.Code -eq 0 -and $w.Text -match 'nothing is connected' -and $w.Text -match 'upload .*BaseGame144\.zip in 256 MB parts' -and $w.Text -match 'Administrators and SYSTEM only' -and $w.Text -match 'Install-DauntlessServer\.ps1 -Mode Public') (($w.Out | Select-Object -Last 6) -join ' | ')

    Write-Host '== upload to a stand-in server folder'
    $base = $t + @('-GameZip', $zip, '-TestZipSha256', $zipHash, '-ChunkSizeMB', '8', '-RestoreFrom', $backup, '-OwnerName', 'Tester', '-ServerName', 'Test server', '-AdminIp', '198.51.100.20,198.51.100.21')
    $r1 = Run ($base + @('-TestStopAfterChunks', '2'))
    Check 'first run drops the connection after 2 parts' ($r1.Code -eq 1 -and $r1.Text -match 'TEST: connection dropped after 2 part') (($r1.Out | Select-Object -Last 4) -join ' | ')
    $up = Join-Path $remote 'staging\upload'
    Check 'two verified parts wait on the server' ((Test-Path -LiteralPath (Join-Path $up 'part00000.ok')) -and (Test-Path -LiteralPath (Join-Path $up 'part00001.ok')) -and -not (Test-Path -LiteralPath (Join-Path $up 'part00002')))
    Check 'the kit was unpacked on the server' ((Test-Path -LiteralPath (Join-Path $remote 'staging\kit\Receive-Upload.ps1')) -and (Test-Path -LiteralPath (Join-Path $remote 'staging\kit\Stack.ps1')) -and -not (Test-Path -LiteralPath (Join-Path $remote 'staging\kit\tests')))
    $srcZip = @(Get-ChildItem -LiteralPath (Join-Path $remote 'staging\source') -Filter 'source-*.zip')
    Check 'the source zip (git archive) arrived' ($srcZip.Count -eq 1 -and $r1.Text -match 'server code uploaded and checked')
    $restored = Join-Path $remote "staging\restore\$(Split-Path $backup -Leaf)"
    Check 'the backup arrived' ((Test-Path -LiteralPath (Join-Path $restored 'undaunted.db')) -and (Test-Path -LiteralPath (Join-Path $restored 'secrets\deployserver.env')))
    $acl = Get-Acl -LiteralPath (Join-Path $remote 'staging\restore')
    $who = @($acl.Access | ForEach-Object { $_.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value } | Sort-Object -Unique)
    Check 'the backup folder is closed to other accounts' ($acl.AreAccessRulesProtected -and -not ($who | Where-Object { $_ -notin 'S-1-5-32-544', 'S-1-5-18', (Get-DRCurrentSid) })) ($who -join ',')

    $r2 = Run ($base + @('-TestCorruptChunk', '3'))
    Check 'second run resumes' ($r2.Code -eq 0 -and $r2.Text -match 'resuming: 2 of 5 part') (($r2.Out | Select-Object -Last 6) -join ' | ')
    Check 'a part damaged in transit is sent again' ($r2.Text -match 'part 4 failed its check on the server \(hash\); sending it again')
    $final = Join-Path $up 'BaseGame144.zip'
    Check 'the zip on the server has the right SHA-256' ((Test-Path -LiteralPath $final) -and (Get-DRSha256 $final).ToLowerInvariant() -eq $zipHash)
    Check 'no parts left behind' (@(Get-ChildItem -LiteralPath $up -Filter 'part*').Count -eq 0)
    $cmdLine = @($r2.Out | Where-Object { $_ -match "^\s+& '.*Install-DauntlessServer\.ps1'" }) | Select-Object -Last 1
    Check 'the installer command is complete' ($cmdLine -match "-Mode 'Public'" -and $cmdLine -match "-GameZip '[^']*BaseGame144\.zip'" -and $cmdLine -match "-SourceZip '[^']*source-[0-9a-f]{12}\.zip' -SourceCommit '[0-9a-f]{40}'" -and $cmdLine -match "-RestoreFrom '[^']*2026-10-01_200000'" -and $cmdLine -match "-OwnerName 'Tester'" -and $cmdLine -match "-ServerName 'Test server'" -and $cmdLine -match "-PublicHost 'test.invalid' -GatewayPort 443" -and $cmdLine -match "-AdminIp '198.51.100.20','198.51.100.21'") $cmdLine
    $tokens = $null; $errors = $null
    [void][Management.Automation.Language.Parser]::ParseInput(($cmdLine.Trim()), [ref]$tokens, [ref]$errors)
    Check 'the installer command is valid PowerShell' ($errors.Count -eq 0)

    $r3 = Run ($base + @('-Chat', 'On'))
    Check 'a repeated run has nothing left to upload' ($r3.Code -eq 0 -and $r3.Text -match 'already on the server and verified' -and $r3.Text -notmatch 'verified on the server \(')
    $chatLine = @($r3.Out | Where-Object { $_ -match "^\s+& '.*Install-DauntlessServer\.ps1'" }) | Select-Object -Last 1
    Check '-Chat On goes to the installer' ($chatLine -match "-Chat 'On'") $chatLine

    Write-Host '== chat on its own'
    $c1 = Run ($t + @('-Chat', 'On'))
    Check '-Chat On alone runs Set-Chat.ps1 -On on the server, nothing else' ($c1.Code -eq 0 -and $c1.Text -match "& '[^']*\\bin\\Set-Chat\.ps1' -On" -and $c1.Text -notmatch 'Uploading') (($c1.Out | Select-Object -Last 4) -join ' | ')
    $c2 = Run @('-Server', '203.0.113.7', '-KeyFile', $dummyKey, '-Chat', 'Off', '-WhatIf')
    Check '-Chat Off -WhatIf names Set-Chat.ps1 -Off' ($c2.Code -eq 0 -and $c2.Text -match 'Set-Chat\.ps1 -Off') (($c2.Out | Select-Object -Last 4) -join ' | ')

    Write-Host '== a part damaged after it was verified'
    $zip2 = Join-Path $WorkDir 'zip2\BaseGame144.zip'
    New-Item -ItemType Directory -Force -Path (Split-Path $zip2) | Out-Null
    New-RandomFile $zip2 (20MB) 3
    $hash2 = (Get-DRSha256 $zip2).ToLowerInvariant()
    $b2 = @('-Server', 'test.invalid', '-TestTargetDir', $remote, '-GameZip', $zip2, '-TestZipSha256', $hash2, '-ChunkSizeMB', '4', '-OwnerName', 'Tester')
    $s1 = Run ($b2 + @('-TestStopAfterChunks', '3'))
    Check 'a different zip starts a new upload' ($s1.Code -eq 1 -and -not (Test-Path -LiteralPath $final) -and (Test-Path -LiteralPath (Join-Path $up 'part00002.ok')))
    $f = [IO.File]::Open((Join-Path $up 'part00001'), 'Open', 'ReadWrite'); try { $f.Position = 1000; $f.WriteByte(7) } finally { $f.Dispose() }
    $s2 = Run $b2
    Check 'assembly finds the damaged part and it is sent again' ($s2.Code -eq 0 -and $s2.Text -match 'sending 1 part\(s\) again: 2' -and (Get-DRSha256 $final).ToLowerInvariant() -eq $hash2) (($s2.Out | Select-Object -Last 6) -join ' | ')

    Write-Host '== working-tree source'
    $s3 = Run @('-Server', 'test.invalid', '-TestTargetDir', (Join-Path $WorkDir 'server2'), '-WorkingTree', '-OwnerName', 'Tester')
    $wz = @(Get-ChildItem -LiteralPath (Join-Path $WorkDir 'server2\staging\source') -Filter 'source-*.zip') | Select-Object -First 1
    Check 'working-tree source uploaded' ($s3.Code -eq 0 -and $wz -and $s3.Text -match '-worktree') (($s3.Out | Select-Object -Last 4) -join ' | ')
    if ($wz) {
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $z = [IO.Compression.ZipFile]::OpenRead($wz.FullName)
        try { $names = @($z.Entries | ForEach-Object { $_.FullName }) } finally { $z.Dispose() }
        Check 'the working tree zip has the server packages' (($names -contains 'legacy/UndauntedMetagame/package.json') -and ($names -contains 'deploy/windows-server/Install-DauntlessServer.ps1'))
        $leak = @($names | Where-Object { $_ -match '(^|/)\.env($|\.)' -and $_ -notmatch '\.env\.example$' -or $_ -match '\.(key|pem|db)$' -or $_ -match '(^|/)node_modules/' -or $_ -match '(^|/)dist/' })
        Check 'no .env, key, database, node_modules or dist in it' ($leak.Count -eq 0) (($leak | Select-Object -First 5) -join ', ')
    }
} finally {
    Remove-Item -LiteralPath $WorkDir -Recurse -Force -ErrorAction SilentlyContinue
}
Write-Host ''
Write-Host "deploy-remote test: $script:Pass passed, $script:Fail failed" -ForegroundColor $(if ($script:Fail) { 'Red' } else { 'Green' })
exit $(if ($script:Fail) { 1 } else { 0 })
