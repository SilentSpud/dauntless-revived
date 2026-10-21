<#
.SYNOPSIS
    Deploys a Dauntless Revived server to a Windows Server over SSH, from the admin's own PC.

.DESCRIPTION
    Uses Windows' built-in OpenSSH client (ssh.exe, scp.exe) with a key file only: batch mode, no
    password or keyboard prompts, and the server's host key pinned in a known_hosts file next to the key
    (the first connection records it, every later one must match).

      1. Checks the server: Windows PowerShell 5.1, administrator session.
      2. Uploads this kit and the server code: a git archive of the current commit (or, with
         -WorkingTree, the working tree without ignored files and secrets), or -Source GitHub -Ref <ref>
         to let the server download a pushed ref itself; and the service binaries for that code
         (-ServerBinaries: the CI download of that commit, or by default this checkout's
         rust\target\release), checked by their SHA-256 after the upload.
      3. -RestoreFrom: uploads a backup folder (database and keys) into a folder only Administrators and
         SYSTEM can read, and deletes it from the server after a successful install.
      4. -GameZip: uploads the 10.5 GB game zip in parts (-ChunkSizeMB, default 256). The zip is checked
         against the pinned SHA-256 here first; each part is checked on the server after it arrives, and
         the whole file again when it is put together. An interrupted upload continues where it stopped
         when you run the same command again. -GameZipUrl instead lets the server download it.
      5. Runs Install-DauntlessServer.ps1 on the server (Update-DauntlessServer.ps1 with -Update) and
         shows its output as it runs.
      6. Prints the public address and the certificate fingerprint, and checks from this PC that the
         server answers through its public gateway with that certificate.

    Other uses:  -InviteFor <name>  makes an invite on the server and prints it;  -Status  shows the
    server's status.  -UploadOnly stops before running the installer.  -Chat On|Off  with an install
    passes the chat setting to the installer; on its own it switches chat on the server with
    Set-Chat.ps1 (which restarts the stack: do it when nobody is playing).

    No key, token or .env value is printed. The backup you upload with -RestoreFrom contains keys: it
    travels only inside the SSH connection.

.EXAMPLE
    .\Deploy-Remote.ps1 -Server 203.0.113.7 -GameZip D:\BaseGame144.zip -RestoreFrom C:\dr\backups\2026-10-01_200000 -AdminIp 198.51.100.20
.EXAMPLE
    .\Deploy-Remote.ps1 -Server 203.0.113.7 -InviteFor Alex
.EXAMPLE
    .\Deploy-Remote.ps1 -Server 203.0.113.7 -Update
.EXAMPLE
    .\Deploy-Remote.ps1 -Server 203.0.113.7 -Chat On
#>
[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)][string]$Server,
    [string]$User = 'Administrator',
    [string]$KeyFile = 'C:\dr\data\ssh\dauntless_deploy',
    [string]$KnownHostsFile,
    # The server's SSH host-key fingerprint (SHA256:..., from the VPS provider's console). On a first
    # connection it is verified and pinned, so a machine in the middle cannot receive the upload. Without
    # it, a first connection with -RestoreFrom (which carries the database and keys) is refused.
    [string]$HostKeyFingerprint,
    [ValidateRange(1, 65535)][int]$SshPort = 22,
    [ValidateSet('Public', 'Private')][string]$Mode = 'Public',
    [string]$GameZip,
    [string]$GameZipUrl,
    [string]$RestoreFrom,
    [string]$OwnerName,
    [string]$ServerName,
    [string]$PublicHost,
    [ValidateRange(1, 65535)][int]$GatewayPort = 443,
    [string[]]$AdminIp,
    [switch]$KeepRdpOpen,
    [string]$InstallRoot = 'C:\DauntlessRevived',
    [ValidateSet('Upload', 'GitHub')][string]$Source = 'Upload',
    [string]$Ref,
    [switch]$WorkingTree,
    # The service binaries for the code (metagame.exe and the rest): a CI server-x86_64-pc-windows-msvc
    # download. Default: rust\target\release in this checkout, when it holds a build.
    [string]$ServerBinaries,
    [switch]$InteractiveSession,
    [ValidateSet('On', 'Off')][string]$Chat,
    [switch]$Update,
    [switch]$UploadOnly,
    [string]$InviteFor,
    [switch]$Status,
    [ValidateRange(1, 2048)][int]$ChunkSizeMB = 256,
    # Tests without a server: a local folder stands in for the server (nothing is installed there).
    [string]$TestTargetDir,
    [string]$TestZipSha256,
    [int]$TestStopAfterChunks = 0,
    [int]$TestCorruptChunk = -1
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
. "$PSScriptRoot\DauntlessServer.Common.ps1"

$script:Test = [bool]$TestTargetDir
$script:ScpLegacy = $null      # $true: scp -O with C:/ paths; $false: SFTP with /C:/ paths; $null: not known yet
$script:Uploaded = 0

# ---------------------------------------------------------------------------------------------
# Transport: SSH, or a local folder in tests. Remote commands are PowerShell text sent as
# -EncodedCommand, so no shell on either side re-interprets quotes. The server's default SSH shell
# (cmd.exe or PowerShell) does not matter.
# ---------------------------------------------------------------------------------------------
function Get-SshArgs {
    return @('-i', $KeyFile, '-p', "$SshPort", '-T',
        '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', 'PasswordAuthentication=no', '-o', 'KbdInteractiveAuthentication=no',
        '-o', 'StrictHostKeyChecking=accept-new', '-o', "UserKnownHostsFile=$KnownHostsFile",
        '-o', 'IPQoS=none', '-o', 'ConnectTimeout=20', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=8')
}

# On a first connection, verify the server's SSH host key against -HostKeyFingerprint and pin it into
# known_hosts, so the initial upload (which may include a backup with the database and all keys) cannot
# be received by a machine in the middle. Once a host is pinned this is a no-op.
function Confirm-DRHostKey {
    $sshKeygen = Join-Path $env:WINDIR 'System32\OpenSSH\ssh-keygen.exe'
    $sshKeyscan = Join-Path $env:WINDIR 'System32\OpenSSH\ssh-keyscan.exe'
    $hostToken = if ($SshPort -eq 22) { $Server } else { "[$Server]:$SshPort" }
    $known = $false
    if ((Test-Path -LiteralPath $KnownHostsFile) -and (Test-Path -LiteralPath $sshKeygen)) {
        $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        try { $f = & $sshKeygen -F $hostToken -f $KnownHostsFile 2>$null } finally { $ErrorActionPreference = $old }
        $known = [bool](@($f | Where-Object { $_ -and $_ -notmatch '^\s*#' }).Count)
    }
    if ($known) {
        if ($HostKeyFingerprint) { Write-DRInfo "server host key already pinned in $KnownHostsFile" }
        return
    }
    if (-not $HostKeyFingerprint) {
        if ($RestoreFrom) { Stop-DR "First connection to $Server and no -HostKeyFingerprint: refusing to upload the backup (it holds the database and every key) to an unverified host. Get the SHA256:... host-key fingerprint from your VPS provider's console and pass -HostKeyFingerprint, or connect once without -RestoreFrom." }
        Write-DRWarn "first connection to ${Server}: trusting the host key it presents (StrictHostKeyChecking=accept-new). Pass -HostKeyFingerprint SHA256:... from the provider console to verify it."
        return
    }
    if (-not (Test-Path -LiteralPath $sshKeyscan) -or -not (Test-Path -LiteralPath $sshKeygen)) { Stop-DR 'ssh-keyscan.exe / ssh-keygen.exe not found. Install the OpenSSH client.' }
    $want = $HostKeyFingerprint.Trim()
    if ($want -notmatch '^SHA256:') { $want = "SHA256:$want" }
    if ($want -cnotmatch '^SHA256:[A-Za-z0-9+/]{43}$') { Stop-DR "-HostKeyFingerprint must look like SHA256:xxxxxxxx... (from the provider console)." }
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $scan = & $sshKeyscan -p $SshPort -T 20 $Server 2>$null } finally { $ErrorActionPreference = $old }
    $scan = @($scan | Where-Object { $_ -and $_ -notmatch '^\s*#' })
    if (-not $scan.Count) { Stop-DR "Could not fetch a host key from ${Server}:$SshPort (ssh-keyscan). Check the address and that sshd is reachable." }
    $matched = $null
    foreach ($line in $scan) {
        $tmp = [IO.Path]::GetTempFileName()
        try {
            [IO.File]::WriteAllText($tmp, ($line + "`n"))
            $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
            try { $fpOut = & $sshKeygen -l -f $tmp 2>$null } finally { $ErrorActionPreference = $old }
        } finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
        $fp = @($fpOut | ForEach-Object { ($_ -split '\s+')[1] } | Where-Object { $_ -match '^SHA256:' }) | Select-Object -First 1
        if ($fp -ceq $want) { $matched = $line; break }
    }
    if (-not $matched) { Stop-DR "The host key ${Server}:$SshPort presents does not match -HostKeyFingerprint $want. Not connecting (a machine in the middle, or the wrong fingerprint)." }
    Add-Content -LiteralPath $KnownHostsFile -Value $matched -Encoding ASCII
    Write-DROk "server host key verified against -HostKeyFingerprint and pinned in $KnownHostsFile"
}

function Get-EncodedCommand([string]$Script) {
    $full = "`$ProgressPreference = 'SilentlyContinue'; `$ErrorActionPreference = 'Stop'`r`n" + $Script
    $b64 = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($full))
    if ($b64.Length -gt 7000) { throw 'internal: remote command too long' }
    return $b64
}

# Runs PowerShell text on the server. -Stream shows every line as it comes; otherwise the lines are
# returned. Either way the exit code is in $script:RemoteExit.
function Invoke-Remote([string]$Script, [switch]$Stream) {
    $b64 = Get-EncodedCommand $Script
    $psArgs = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-EncodedCommand', $b64)
    $lines = New-Object System.Collections.Generic.List[string]
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try {
        if ($script:Test) { $exe = (Join-Path $env:WINDIR 'System32\WindowsPowerShell\v1.0\powershell.exe'); $all = $psArgs }
        else { $exe = $script:Ssh; $all = @(Get-SshArgs) + @("$User@$Server", 'powershell.exe') + $psArgs }
        & $exe @all 2>&1 | ForEach-Object {
            $l = if ($_ -is [Management.Automation.ErrorRecord]) { "$($_.Exception.Message)" } else { "$_" }
            if ($Stream) { if ($l -notmatch '^DRJSON:') { Write-Host $l } }
            $lines.Add($l)
        }
        $script:RemoteExit = $LASTEXITCODE
    } finally { $ErrorActionPreference = $old }
    return ,$lines
}

function Get-RemoteJson($Lines) {
    $j = @($Lines | Where-Object { $_ -match '^DRJSON:' }) | Select-Object -Last 1
    if (-not $j) { return $null }
    return ($j.Substring(7) | ConvertFrom-Json)
}

function ConvertTo-ScpPath([string]$WindowsPath, [bool]$Legacy) {
    $p = $WindowsPath -replace '\\', '/'
    if ($Legacy) { return $p }
    return "/$p"
}

# Copies a local file (or folder, -Recurse) to a path on the server.
function Send-Item([string]$Local, [string]$RemotePath, [switch]$Recurse) {
    if ($script:Test) {
        if ($Recurse) { Copy-Item -LiteralPath $Local -Destination $RemotePath -Recurse -Force }
        else { Copy-Item -LiteralPath $Local -Destination $RemotePath -Force }
        return
    }
    $modes = if ($null -eq $script:ScpLegacy) { @($false, $true) } else { @($script:ScpLegacy) }
    foreach ($legacy in $modes) {
        $scpArgs = @('-i', $KeyFile, '-P', "$SshPort", '-q',
            '-o', 'IdentitiesOnly=yes', '-o', 'BatchMode=yes', '-o', 'PasswordAuthentication=no', '-o', 'KbdInteractiveAuthentication=no',
            '-o', 'StrictHostKeyChecking=accept-new', '-o', "UserKnownHostsFile=$KnownHostsFile", '-o', 'ConnectTimeout=20', '-o', 'ServerAliveInterval=15')
        if ($legacy) { $scpArgs += '-O' }
        if ($Recurse) { $scpArgs += '-r' }
        $scpArgs += @($Local, ("{0}@{1}:{2}" -f $User, $Server, (ConvertTo-ScpPath $RemotePath $legacy)))
        $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        try { $out = & $script:Scp @scpArgs 2>&1 | ForEach-Object { "$_" }; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $old }
        if ($code -eq 0) { $script:ScpLegacy = $legacy; return }
        $last = ($out | Where-Object { $_.Trim() } | Select-Object -Last 1)
        if ($modes.Count -gt 1 -and -not $legacy) { Write-DRInfo "scp (SFTP mode) failed ($last); trying the classic scp protocol" }
    }
    throw "scp to $RemotePath failed (exit code $code): $last"
}

# ---------------------------------------------------------------------------------------------
# Chunked upload of a large file (the server side is Receive-Upload.ps1 in the kit).
# ---------------------------------------------------------------------------------------------
function Get-ChunkHashes([string]$Path, [long]$ChunkSize) {
    $len = (Get-Item -LiteralPath $Path).Length
    $n = [int][math]::Ceiling($len / [double]$ChunkSize)
    $hashes = New-Object string[] $n
    $whole = [Security.Cryptography.SHA256]::Create()
    $buf = New-Object byte[] (4MB)
    $fs = [IO.File]::OpenRead($Path)
    $done = 0L; $next = 1
    try {
        for ($i = 0; $i -lt $n; $i++) {
            $part = [Security.Cryptography.SHA256]::Create()
            $left = [long][math]::Min($ChunkSize, $len - $i * $ChunkSize)
            while ($left -gt 0) {
                $r = $fs.Read($buf, 0, [int][math]::Min($buf.Length, $left))
                if ($r -le 0) { throw 'unexpected end of file' }
                [void]$whole.TransformBlock($buf, 0, $r, $null, 0)
                [void]$part.TransformBlock($buf, 0, $r, $null, 0)
                $left -= $r; $done += $r
            }
            [void]$part.TransformFinalBlock((New-Object byte[] 0), 0, 0)
            $hashes[$i] = ConvertTo-DRHex $part.Hash
            $part.Dispose()
            if ($len -gt 1GB -and ($done / $len) * 10 -ge $next) { Write-DRInfo ('  hashed {0}%' -f [int](100 * $done / $len)); $next++ }
        }
        [void]$whole.TransformFinalBlock((New-Object byte[] 0), 0, 0)
        return [pscustomobject]@{ Size = $len; Sha256 = (ConvertTo-DRHex $whole.Hash); Chunks = $hashes; Count = $n }
    } finally { $fs.Dispose(); $whole.Dispose() }
}

function Write-ChunkFile([string]$Source, [long]$Offset, [long]$Length, [string]$Dest) {
    $in = [IO.File]::OpenRead($Source)
    $out = [IO.File]::Create($Dest)
    try {
        [void]$in.Seek($Offset, [IO.SeekOrigin]::Begin)
        $buf = New-Object byte[] (4MB)
        $left = $Length
        while ($left -gt 0) {
            $r = $in.Read($buf, 0, [int][math]::Min($buf.Length, $left))
            if ($r -le 0) { throw 'unexpected end of file' }
            $out.Write($buf, 0, $r); $left -= $r
        }
    } finally { $in.Dispose(); $out.Dispose() }
}

function Invoke-Receive([string]$Action, [int]$Index = -1) {
    $cmd = "& $(ConvertTo-DRPsLiteral (Join-Path $RemoteKit 'Receive-Upload.ps1')) -Action $Action -Dir $(ConvertTo-DRPsLiteral $RemoteUpload)"
    if ($Index -ge 0) { $cmd += " -Index $Index" }
    $lines = Invoke-Remote $cmd
    $j = Get-RemoteJson $lines
    if (-not $j -or ($j.PSObject.Properties['error'] -and $j.error -and -not $j.PSObject.Properties['complete'])) {
        $why = if ($j) { $j.error } else { ($lines | Select-Object -Last 3) -join ' | ' }
        throw "Receive-Upload.ps1 $Action failed on the server: $why"
    }
    return $j
}

function Send-Part($Info, [int]$I, [long]$ChunkSize, [string]$TempDir) {
    $offset = [long]$I * $ChunkSize
    $length = [long][math]::Min($ChunkSize, $Info.Size - $offset)
    $name = 'part{0:D5}' -f $I
    for ($try = 1; $try -le 3; $try++) {
        if ($TestStopAfterChunks -gt 0 -and $script:Uploaded -ge $TestStopAfterChunks) { throw "TEST: connection dropped after $script:Uploaded part(s)" }
        $tmp = Join-Path $TempDir $name
        Write-ChunkFile $GameZip $offset $length $tmp
        try { Send-Item $tmp (Join-Path $RemoteUpload $name) } finally { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
        if ($script:Test -and $TestCorruptChunk -eq $I -and $try -eq 1) {
            $f = [IO.File]::Open((Join-Path $RemoteUpload $name), 'Open', 'ReadWrite'); try { $b = $f.ReadByte(); $f.Position = 0; $f.WriteByte(($b -bxor 0xFF)) } finally { $f.Dispose() }
        }
        $v = Invoke-Receive 'Verify' $I
        if ($v.ok) {
            $script:Uploaded++
            Write-DROk ('part {0}/{1} ({2:N0} MB) verified on the server' -f ($I + 1), $Info.Count, ($length / 1MB))
            return
        }
        Write-DRWarn "part $($I + 1) failed its check on the server ($($v.reason)); sending it again"
    }
    throw "part $($I + 1) failed its check three times"
}

function Send-GameZip {
    $chunk = [long]$ChunkSizeMB * 1MB
    Write-DRInfo "hashing $GameZip locally (one read of the whole file) ..."
    $info = Get-ChunkHashes $GameZip $chunk
    $expected = if ($TestZipSha256) { $TestZipSha256.ToLowerInvariant() } else { $DRPinned.GameZipSha256.ToLowerInvariant() }
    if ($info.Sha256 -ne $expected) { Stop-DR "$GameZip is not the verified 1.4.4 build (SHA-256 $($info.Sha256)). Not uploading it." }
    Write-DROk ('the zip is the verified build ({0:N2} GB, {1} parts of {2} MB)' -f ($info.Size / 1GB), $info.Count, $ChunkSizeMB)

    $manifest = [ordered]@{ name = 'BaseGame144.zip'; size = $info.Size; sha256 = $info.Sha256; chunkSize = $chunk; chunks = [object[]]@($info.Chunks) }
    $tempDir = Join-Path ([IO.Path]::GetTempPath()) "DauntlessRevived-upload-$PID"
    New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
    try {
        $mf = Join-Path $tempDir 'upload.json.incoming'
        Write-DRText -Path $mf -Text ($manifest | ConvertTo-Json -Compress -Depth 3)
        Send-Item $mf (Join-Path $RemoteUpload 'upload.json.incoming')
        $state = Invoke-Receive 'Begin'
        if ($state.complete) { Write-DROk 'the game zip is already on the server and verified'; return $state.path }
        $have = @($state.verified | ForEach-Object { [int]$_ })
        $start = [int]$state.assembled
        if ($have.Count -or $start) { Write-DROk "resuming: $($have.Count + $start) of $($info.Count) part(s) already on the server" }
        for ($i = $start; $i -lt $info.Count; $i++) { if ($have -notcontains $i) { Send-Part $info $i $chunk $tempDir } }
        for ($round = 1; $round -le 4; $round++) {
            Write-DRInfo 'putting the parts together on the server and checking the whole file ...'
            $a = Invoke-Receive 'Assemble'
            if ($a.complete) { Write-DROk 'the game zip is on the server and matches the pinned SHA-256'; return $a.path }
            $again = @(@($a.bad) + @($a.missing) | Where-Object { $null -ne $_ } | ForEach-Object { [int]$_ } | Sort-Object -Unique)
            if ($a.PSObject.Properties['error'] -and $a.error) {
                Write-DRWarn "the assembled file did not match ($($a.error))"
                $again = @(0..($info.Count - 1))
            }
            if (-not $again.Count) { throw 'the server could not put the zip together and did not say why' }
            Write-DRWarn "sending $($again.Count) part(s) again: $(($again | ForEach-Object { $_ + 1 }) -join ', ')"
            foreach ($i in $again) { Send-Part $info $i $chunk $tempDir }
        }
        throw 'the zip could not be put together on the server after 4 attempts'
    } finally { Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue }
}

# ---------------------------------------------------------------------------------------------
# Source code archive: git archive of HEAD, or the working tree without ignored files and secrets.
# ---------------------------------------------------------------------------------------------
function New-SourceZip([string]$OutFile) {
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
    if (-not (Test-Path -LiteralPath (Join-Path $repo '.git'))) { Stop-DR "$repo is not a git checkout; use -Source GitHub -Ref <ref>." }
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try {
        $commit = (& git.exe -C $repo rev-parse HEAD 2>$null | Out-String).Trim()
        $dirty = [bool]((& git.exe -C $repo status --porcelain 2>$null | Out-String).Trim())
    } finally { $ErrorActionPreference = $old }
    if ($commit -notmatch '^[0-9a-f]{40}$') { Stop-DR 'Could not read the current commit (git rev-parse HEAD).' }
    # Public mode ships the git archive of HEAD by default. If HEAD does not yet contain the gateway and
    # content server (they may be uncommitted), the install fails on the server only after hours of
    # uploading. Catch it here, before the backup and the ~10.5 GB game zip go up.
    if (-not $WorkingTree -and $Mode -eq 'Public' -and -not $script:Test) {
        foreach ($need in 'legacy/UndauntedGateway/package.json', 'legacy/UndauntedContent/package.json') {
            $old2 = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
            try { & git.exe -C $repo cat-file -e "HEAD:$need" 2>$null; $present = ($LASTEXITCODE -eq 0) } finally { $ErrorActionPreference = $old2 }
            if (-not $present) { Stop-DR "HEAD does not contain $need, which public mode needs. Commit and push your work, or add -WorkingTree to deploy your current files." }
        }
    }
    if (-not $WorkingTree) {
        if ($dirty) { Write-DRWarn 'the checkout has uncommitted changes; they are NOT deployed (commit them, or use -WorkingTree)' }
        $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
        try { & git.exe -C $repo archive --format=zip -o $OutFile HEAD 2>&1 | Out-Null; $code = $LASTEXITCODE } finally { $ErrorActionPreference = $old }
        if ($code -ne 0 -or -not (Test-Path -LiteralPath $OutFile)) { Stop-DR 'git archive failed.' }
        return $commit
    }
    $old = $ErrorActionPreference; $ErrorActionPreference = 'Continue'
    try { $files = @(& git.exe -C $repo ls-files -co --exclude-standard 2>$null) } finally { $ErrorActionPreference = $old }
    $keep = @($files | Where-Object {
        $_ -and $_ -notmatch '(^|/)(node_modules|dist|build|out|\.vite)/' -and
        $_ -notmatch '(^|/)\.env($|\.)' -and $_ -notmatch '\.(key|pem|db|db-journal|db-wal|db-shm|log)$'
    })
    $keep += @($files | Where-Object { $_ -match '(^|/)\.env\.example$' })
    # git still lists tracked files that were deleted in the working tree.
    $keep = @($keep | Sort-Object -Unique | Where-Object { Test-Path -LiteralPath (Join-Path $repo $_) -PathType Leaf })
    $list = "$OutFile.list"
    [IO.File]::WriteAllLines($list, [string[]]$keep, (New-Object System.Text.UTF8Encoding($false)))
    try {
        $code = Invoke-DRNative -FilePath (Get-DRTar) -Arguments @('--format', 'zip', '-cf', $OutFile, '-T', $list) -WorkingDirectory $repo -LogBase "$OutFile.tar"
        if ($code -ne 0) { Show-DRLogTail "$OutFile.tar" 5; Stop-DR "packing the working tree failed (tar exit code $code)" }
    } finally { Remove-Item -LiteralPath $list, "$OutFile.tar.out.log", "$OutFile.tar.err.log" -Force -ErrorAction SilentlyContinue }
    return "$commit-worktree"
}

# ---------------------------------------------------------------------------------------------
$exitCode = 0
try {
    Write-DRStep "Dauntless Revived remote deployment$(if ($script:Test) { '  (TEST: a local folder stands in for the server)' })"
    # --- arguments ---------------------------------------------------------------------------
    $Server = $Server.Trim().ToLowerInvariant()
    if (-not (Test-DRHost $Server)) { Stop-DR "-Server '$Server' must be an IPv4 address or a DNS name." }
    if ($User -notmatch '^[A-Za-z0-9._-]{1,64}$') { Stop-DR "-User '$User' is not a plain account name." }
    if ($InstallRoot -notmatch '^[A-Za-z]:\\[A-Za-z0-9_.\\-]+$') { Stop-DR '-InstallRoot must be an absolute path without spaces (for example C:\DauntlessRevived).' }
    $InstallRoot = $InstallRoot.TrimEnd('\')
    $testOnly = @('TestZipSha256', 'TestStopAfterChunks', 'TestCorruptChunk') | Where-Object { $PSBoundParameters.ContainsKey($_) }
    if ($testOnly -and -not $script:Test) { Stop-DR "-$($testOnly[0]) only works with -TestTargetDir." }
    if ($TestZipSha256 -and $TestZipSha256 -notmatch '^[0-9A-Fa-f]{64}$') { Stop-DR '-TestZipSha256 must be 64 hex characters.' }
    $actions = @(@($Update, $Status, [bool]$InviteFor) | Where-Object { $_ })
    if ($actions.Count -gt 1) { Stop-DR 'Use one of -Update, -Status or -InviteFor.' }
    $isInstall = -not ($Update -or $Status -or $InviteFor)
    # -Chat with an install goes to the installer; on its own it runs Set-Chat.ps1 on the server
    if ($Chat -and -not $isInstall) { Stop-DR '-Chat goes with an install, or on its own (it then switches chat with Set-Chat.ps1 on the server).' }
    $chatOnly = [bool]$Chat -and -not ($OwnerName -or $RestoreFrom -or $GameZip -or $GameZipUrl -or $UploadOnly -or $Ref -or $WorkingTree -or $ServerName -or $AdminIp)
    if ($chatOnly) { $isInstall = $false }
    if ($Chat -eq 'On' -and $isInstall -and $Mode -eq 'Private') { Stop-DR 'Chat works in public mode only for now (private mode comes later, roadmap 3.10).' }
    if ($GameZip -and $GameZipUrl) { Stop-DR 'Use -GameZip or -GameZipUrl, not both.' }
    if ($GameZipUrl -and $GameZipUrl -notmatch '^https://\S+$') { Stop-DR '-GameZipUrl must be an https:// URL.' }
    if (-not $isInstall -and ($GameZip -or $GameZipUrl -or $RestoreFrom -or $OwnerName)) { Stop-DR '-GameZip, -GameZipUrl, -RestoreFrom and -OwnerName belong to an install (not -Update, -Status or -InviteFor).' }
    if ($GameZip) { if (-not (Test-Path -LiteralPath $GameZip -PathType Leaf)) { Stop-DR "Game zip not found: $GameZip" }; $GameZip = (Resolve-Path -LiteralPath $GameZip).Path }
    if ($RestoreFrom) {
        if (-not (Test-Path -LiteralPath $RestoreFrom -PathType Container)) { Stop-DR "-RestoreFrom folder not found: $RestoreFrom" }
        $RestoreFrom = (Resolve-Path -LiteralPath $RestoreFrom).Path.TrimEnd('\')
        foreach ($f in 'undaunted.db', 'secrets\metagame.env', 'secrets\deployserver.env') {
            if (-not (Test-Path -LiteralPath (Join-Path $RestoreFrom $f))) { Stop-DR "-RestoreFrom: $f is missing in $RestoreFrom" }
        }
        if ((Split-Path $RestoreFrom -Leaf) -notmatch '^[A-Za-z0-9_.-]{1,64}$') { Stop-DR '-RestoreFrom must be a backup folder with a plain name (like 2026-10-01_200000).' }
    }
    if ($OwnerName -and -not (Test-DRUsername $OwnerName)) { Stop-DR '-OwnerName must be 3-16 letters, digits or _.' }
    if ($isInstall -and -not $UploadOnly -and -not $OwnerName -and -not $RestoreFrom) { Stop-DR 'An install needs -OwnerName <your username> (new server) or -RestoreFrom <backup folder> (moving a server).' }
    if ($ServerName -and -not (Test-DRServerName $ServerName)) { Stop-DR '-ServerName must be 1-64 characters without line breaks, quotes or backslashes.' }
    if ($PublicHost -and -not (Test-DRHost $PublicHost.ToLowerInvariant())) { Stop-DR '-PublicHost must be an IPv4 address or a DNS name.' }
    # "a,b" arrives as one string through powershell -File; accept both forms.
    $AdminIp = @($AdminIp | ForEach-Object { "$_" -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
    foreach ($a in @($AdminIp)) { if ($a -and -not (Test-DRIPv4Cidr $a)) { Stop-DR "-AdminIp '$a' must be an IPv4 address or IPv4/prefix." } }
    if ($InviteFor -and $InviteFor -match '[\x00-\x1f"''`$]') { Stop-DR '-InviteFor must be plain text.' }
    if ($Mode -eq 'Public' -and -not $PublicHost -and $isInstall) { $PublicHost = $Server }
    if ($Source -eq 'GitHub' -and $WorkingTree) { Stop-DR '-WorkingTree uploads local files; it does not go with -Source GitHub.' }
    if ($isInstall -and $Mode -eq 'Public' -and -not $AdminIp -and -not $KeepRdpOpen) { Write-DRWarn 'no -AdminIp: the installer turns off any internet-open Remote Desktop rule (administer over this key-only SSH). Pass -AdminIp <your IP> to keep RDP from your address, or -KeepRdpOpen to leave it open.' }

    if ($script:Test) {
        $InstallRoot = [IO.Path]::GetFullPath($TestTargetDir).TrimEnd('\')
        New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
    } else {
        $script:Ssh = Join-Path $env:WINDIR 'System32\OpenSSH\ssh.exe'
        $script:Scp = Join-Path $env:WINDIR 'System32\OpenSSH\scp.exe'
        foreach ($t in $script:Ssh, $script:Scp) { if (-not (Test-Path -LiteralPath $t)) { Stop-DR "$t not found. Install the OpenSSH client (Settings > Apps > Optional features)." } }
        if (-not (Test-Path -LiteralPath $KeyFile -PathType Leaf)) { Stop-DR "SSH key not found: $KeyFile" }
        $KeyFile = (Resolve-Path -LiteralPath $KeyFile).Path
        if (-not $KnownHostsFile) { $KnownHostsFile = Join-Path (Split-Path $KeyFile -Parent) 'known_hosts' }
        if ($KnownHostsFile -match '\s') { Stop-DR "-KnownHostsFile must be a path without spaces ($KnownHostsFile)." }
        # OpenSSH refuses a private key that other accounts can read; say so before it does.
        $wide = @((Get-Acl -LiteralPath $KeyFile).Access | Where-Object {
            $_.AccessControlType -eq 'Allow' -and "$($_.IdentityReference)" -match '(\\|^)(Everyone|Users|Authenticated Users)$' })
        if ($wide.Count) { Write-DRWarn "$KeyFile is readable by $(@($wide | ForEach-Object { $_.IdentityReference }) -join ', '); ssh will refuse it. Fix: icacls `"$KeyFile`" /inheritance:r /grant:r `"$($env:USERNAME):R`"" }
        Write-DRInfo "server  : $User@${Server}:$SshPort (key $KeyFile, host keys in $KnownHostsFile)"
        if (-not $WhatIfPreference) { Confirm-DRHostKey }
    }
    $RemoteStaging = Join-Path $InstallRoot 'staging'
    $RemoteKit = Join-Path $RemoteStaging 'kit'
    $RemoteUpload = Join-Path $RemoteStaging 'upload'
    $RemoteSource = Join-Path $RemoteStaging 'source'
    $RemoteRestore = Join-Path $RemoteStaging 'restore'
    $RemoteBin = Join-Path $InstallRoot 'bin'

    if ($WhatIfPreference) {
        Write-DRInfo 'what-if: nothing is connected, uploaded or run. The plan:'
        if ($Status) { Write-DRInfo "  run $RemoteBin\Stack.ps1 status and Get-ServerStatus.ps1 on the server" }
        elseif ($chatOnly) { Write-DRInfo "  run $RemoteBin\Set-Chat.ps1 -$Chat on the server (it restarts the stack)" }
        elseif ($InviteFor) { Write-DRInfo "  run $RemoteBin\New-Invite.ps1 -For '$InviteFor' on the server, then check the invite from here" }
        else {
            Write-DRInfo "  upload the kit to $RemoteKit"
            if ($Source -eq 'Upload') { Write-DRInfo "  upload the server code ($(if ($WorkingTree) { 'working tree' } else { 'git archive of HEAD' })) to $RemoteSource" } else { Write-DRInfo "  let the server download $($DRRepo.Url) at $(if ($Ref) { $Ref } else { $DRRepo.PinnedRef })" }
            if ($RestoreFrom) { Write-DRInfo "  upload the backup $RestoreFrom to $RemoteRestore (Administrators and SYSTEM only)" }
            if ($GameZip) { Write-DRInfo "  upload $GameZip in $ChunkSizeMB MB parts to $RemoteUpload (resumable, SHA-256 checked)" }
            if ($GameZipUrl) { Write-DRInfo "  let the server download the game zip from $(([Uri]$GameZipUrl).Host)" }
            if (-not $UploadOnly) { Write-DRInfo "  run $(if ($Update) { 'Update-DauntlessServer.ps1' } else { "Install-DauntlessServer.ps1 -Mode $Mode" }) on the server" }
        }
        exit 0
    }

    # --- the server ----------------------------------------------------------------------------
    Write-DRStep 'Connecting'
    $probe = Invoke-Remote @'
$id = [Security.Principal.WindowsIdentity]::GetCurrent()
$admin = (New-Object Security.Principal.WindowsPrincipal($id)).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
$os = Get-CimInstance Win32_OperatingSystem
'DRJSON:' + (@{ ps = "$($PSVersionTable.PSVersion)"; admin = $admin; os = "$($os.Caption)"; build = "$($os.BuildNumber)"; user = $id.Name; computer = $env:COMPUTERNAME } | ConvertTo-Json -Compress)
'@
    $p = Get-RemoteJson $probe
    if ($script:RemoteExit -ne 0 -or -not $p) {
        ($probe | Select-Object -Last 5) | ForEach-Object { Write-DRInfo $_ }
        Stop-DR "Could not run PowerShell on $Server over SSH (exit code $script:RemoteExit). Check the address, that sshd runs there, and that the public key is in C:\ProgramData\ssh\administrators_authorized_keys."
    }
    Write-DROk "$($p.computer): $($p.os) (build $($p.build)), PowerShell $($p.ps), signed in as $($p.user)"
    if (-not $p.admin -and -not $script:Test) { Stop-DR "The SSH session is not elevated ($($p.user)). Use the Administrator account." }

    if ($Status) {
        [void](Invoke-Remote "& $(ConvertTo-DRPsLiteral (Join-Path $RemoteBin 'Stack.ps1')) status 6>&1; ''; & $(ConvertTo-DRPsLiteral (Join-Path $RemoteBin 'Get-ServerStatus.ps1')) 6>&1" -Stream)
        exit $script:RemoteExit
    }

    if ($chatOnly) {
        $chatCmd = "& $(ConvertTo-DRPsLiteral (Join-Path $RemoteBin 'Set-Chat.ps1')) -$Chat 6>&1; exit `$LASTEXITCODE"
        if ($script:Test) {
            Write-DRStep 'Ready'
            Write-DRInfo 'the command that would run there (not run in a test):'
            Write-Host "   $chatCmd"
            exit 0
        }
        Write-DRStep "Switching chat $($Chat.ToLowerInvariant()) on the server (the stack restarts)"
        [void](Invoke-Remote $chatCmd -Stream)
        if ($script:RemoteExit -ne 0) { Stop-DR 'Set-Chat.ps1 failed on the server (see above).' }
        exit 0
    }

    if ($InviteFor) {
        $lines = Invoke-Remote "& $(ConvertTo-DRPsLiteral (Join-Path $RemoteBin 'New-Invite.ps1')) -For $(ConvertTo-DRPsLiteral $InviteFor) 6>&1; exit `$LASTEXITCODE" -Stream
        if ($script:RemoteExit -ne 0) { Stop-DR 'New-Invite.ps1 failed on the server (see above).' }
        $invite = @($lines | Where-Object { $_ -match '^dauntless-revived://' }) | Select-Object -Last 1
        if ($invite) {
            Write-DRStep 'Checking the invite from this PC'
            & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'Get-ServerStatus.ps1') -Invite $invite | ForEach-Object { Write-Host $_ }
            if ($LASTEXITCODE -ne 0) { Write-DRWarn 'this PC could not reach the server with the invite (see above)' }
        }
        exit 0
    }

    # --- uploads -------------------------------------------------------------------------------
    Write-DRStep 'Uploading the kit'
    $stage = Join-Path ([IO.Path]::GetTempPath()) "DauntlessRevived-deploy-$PID"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    try {
        [void](Invoke-Remote "foreach (`$d in $(ConvertTo-DRPsLiteral $RemoteStaging), $(ConvertTo-DRPsLiteral $RemoteUpload), $(ConvertTo-DRPsLiteral $RemoteSource)) { New-Item -ItemType Directory -Force -Path `$d | Out-Null }; 'DRJSON:{}'")
        if ($script:RemoteExit -ne 0) { Stop-DR "Could not create $RemoteStaging on the server." }
        $kitZip = Join-Path $stage 'kit.zip'
        $kitFiles = @(Get-ChildItem -LiteralPath $PSScriptRoot | Where-Object { $_.Name -ne 'tests' })
        Compress-Archive -LiteralPath ($kitFiles | ForEach-Object { $_.FullName }) -DestinationPath $kitZip -Force
        Send-Item $kitZip (Join-Path $RemoteStaging 'kit.zip')
        $r = Invoke-Remote "`$k = $(ConvertTo-DRPsLiteral $RemoteKit); if (Test-Path -LiteralPath `$k) { Remove-Item -LiteralPath `$k -Recurse -Force }; Expand-Archive -LiteralPath $(ConvertTo-DRPsLiteral (Join-Path $RemoteStaging 'kit.zip')) -DestinationPath `$k -Force; Remove-Item -LiteralPath $(ConvertTo-DRPsLiteral (Join-Path $RemoteStaging 'kit.zip')) -Force; 'DRJSON:' + (@{ files = @(Get-ChildItem -LiteralPath `$k -Recurse -File).Count } | ConvertTo-Json -Compress)"
        $kj = Get-RemoteJson $r
        if ($script:RemoteExit -ne 0 -or -not $kj) { Stop-DR 'Unpacking the kit on the server failed.' }
        Write-DROk "kit unpacked on the server ($($kj.files) files in $RemoteKit)"

        $sourceArgs = @()
        if ($Source -eq 'Upload') {
            Write-DRStep 'Uploading the server code'
            $srcZip = Join-Path $stage 'source.zip'
            $commit = New-SourceZip $srcZip
            Write-DRInfo ('source {0} ({1:N1} MB)' -f $commit, ((Get-Item -LiteralPath $srcZip).Length / 1MB))
            $remoteZip = Join-Path $RemoteSource "source-$($commit.Substring(0, 12)).zip"
            Send-Item $srcZip $remoteZip
            $hashHere = (Get-DRSha256 $srcZip).ToLowerInvariant()
            $hr = Invoke-Remote "'DRJSON:' + (@{ sha = (Get-FileHash -LiteralPath $(ConvertTo-DRPsLiteral $remoteZip) -Algorithm SHA256).Hash.ToLowerInvariant() } | ConvertTo-Json -Compress)"
            $hj = Get-RemoteJson $hr
            if (-not $hj -or $hj.sha -ne $hashHere) { Stop-DR 'The source zip arrived damaged (SHA-256 differs).' }
            Write-DROk "server code uploaded and checked ($commit)"
            $sourceArgs = @('-SourceZip', $remoteZip, '-SourceCommit', $commit)
        } else {
            $sourceArgs = @('-Ref', $(if ($Ref) { $Ref } else { $DRRepo.PinnedRef }))
        }

        # The service binaries, beside the code: the server checks them against SHA256SUMS when it has one
        $binLocal = $null
        if ($ServerBinaries) { $binLocal = [IO.Path]::GetFullPath($ServerBinaries) }
        else {
            $release = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\rust\target\release'))
            if (Test-Path -LiteralPath (Join-Path $release 'metagame.exe')) { $binLocal = $release }
        }
        if ($binLocal) {
            Write-DRStep 'Uploading the service binaries'
            $binStage = Join-Path $stage 'binaries'
            New-Item -ItemType Directory -Force -Path $binStage | Out-Null
            $names = @($DRBinaries | ForEach-Object { "$_.exe" })
            if (Test-Path -LiteralPath (Join-Path $binLocal 'SHA256SUMS')) { $names += 'SHA256SUMS' }
            foreach ($n in $names) {
                if (-not (Test-Path -LiteralPath (Join-Path $binLocal $n))) { Stop-DR "$binLocal has no $n" }
                Copy-Item -LiteralPath (Join-Path $binLocal $n) -Destination $binStage
            }
            $binRemote = Join-Path $RemoteSource 'binaries'
            [void](Invoke-Remote "`$b = $(ConvertTo-DRPsLiteral $binRemote); if (Test-Path -LiteralPath `$b) { Remove-Item -LiteralPath `$b -Recurse -Force }; 'DRJSON:{}'")
            Send-Item $binStage $RemoteSource -Recurse
            $hashesHere = [ordered]@{}
            foreach ($n in $names) { $hashesHere[$n] = Get-DRSha256 (Join-Path $binStage $n) }
            $br = Invoke-Remote "`$b = $(ConvertTo-DRPsLiteral $binRemote); `$h = @{}; foreach (`$f in Get-ChildItem -LiteralPath `$b -File) { `$h[`$f.Name] = (Get-FileHash -LiteralPath `$f.FullName -Algorithm SHA256).Hash }; 'DRJSON:' + (`$h | ConvertTo-Json -Compress)"
            $bj = Get-RemoteJson $br
            foreach ($n in $names) {
                if (-not $bj -or [string]$bj.$n -ne $hashesHere[$n]) { Stop-DR "$n arrived damaged (SHA-256 differs)." }
            }
            Write-DROk "service binaries uploaded and checked ($binLocal)"
            $sourceArgs += @('-ServerBinaries', $binRemote)
        } elseif ($Source -eq 'Upload') {
            Write-DRWarn 'no service binaries given (-ServerBinaries) and none built in rust\target\release: the server must find them in the code'
        }

        $restoreRemote = $null
        if ($RestoreFrom) {
            Write-DRStep 'Uploading the backup (database and keys)'
            $leaf = Split-Path $RestoreFrom -Leaf
            $restoreRemote = Join-Path $RemoteRestore $leaf
            # A folder only Administrators and SYSTEM can open, before anything goes into it (in a test
            # also the current user, who may not be elevated).
            $sids = "'S-1-5-32-544', 'S-1-5-18'"
            if ($script:Test) { $sids += ", '$(Get-DRCurrentSid)'" }
            [void](Invoke-Remote "`$d = $(ConvertTo-DRPsLiteral $RemoteRestore); New-Item -ItemType Directory -Force -Path `$d | Out-Null; `$a = New-Object Security.AccessControl.DirectorySecurity; `$a.SetAccessRuleProtection(`$true, `$false); foreach (`$s in $sids) { `$a.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule((New-Object Security.Principal.SecurityIdentifier(`$s)), 'FullControl', 'ContainerInherit, ObjectInherit', 'None', 'Allow'))) }; (Get-Item -LiteralPath `$d).SetAccessControl(`$a); `$old = $(ConvertTo-DRPsLiteral $restoreRemote); if (Test-Path -LiteralPath `$old) { Remove-Item -LiteralPath `$old -Recurse -Force }; 'DRJSON:{}'")
            if ($script:RemoteExit -ne 0) { Stop-DR 'Could not prepare the restore folder on the server.' }
            Send-Item $RestoreFrom $RemoteRestore -Recurse
            $localCount = @(Get-ChildItem -LiteralPath $RestoreFrom -Recurse -File).Count
            $rr = Invoke-Remote "'DRJSON:' + (@{ files = @(Get-ChildItem -LiteralPath $(ConvertTo-DRPsLiteral $restoreRemote) -Recurse -File).Count; db = (Get-FileHash -LiteralPath (Join-Path $(ConvertTo-DRPsLiteral $restoreRemote) 'undaunted.db') -Algorithm SHA256).Hash } | ConvertTo-Json -Compress)"
            $rj = Get-RemoteJson $rr
            if (-not $rj -or [int]$rj.files -ne $localCount -or $rj.db -ne (Get-DRSha256 (Join-Path $RestoreFrom 'undaunted.db'))) { Stop-DR 'The backup did not arrive complete (file count or database hash differs).' }
            Write-DROk "backup uploaded ($localCount files; database hash checked; contents not shown)"
        }

        $gameArgs = @()
        if ($GameZip) {
            Write-DRStep 'Uploading the game zip'
            $zipRemote = Send-GameZip
            $gameArgs = @('-GameZip', $zipRemote)
        } elseif ($GameZipUrl) {
            $gameArgs = @('-GameZipUrl', $GameZipUrl)
        }
    } finally { Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue }

    # --- install or update ----------------------------------------------------------------------
    # Parameters as name -> value (strings become PowerShell literals, arrays literal lists), switches bare.
    $named = [ordered]@{}
    $switches = @()
    if ($Update) {
        $scriptName = 'Update-DauntlessServer.ps1'
        $named['Root'] = $InstallRoot
        for ($i = 0; $i -lt $sourceArgs.Count; $i += 2) { $named[$sourceArgs[$i].TrimStart('-')] = $sourceArgs[$i + 1] }
    } else {
        $scriptName = 'Install-DauntlessServer.ps1'
        $named['Mode'] = $Mode
        $named['InstallRoot'] = $InstallRoot
        for ($i = 0; $i -lt $sourceArgs.Count; $i += 2) { $named[$sourceArgs[$i].TrimStart('-')] = $sourceArgs[$i + 1] }
        for ($i = 0; $i -lt $gameArgs.Count; $i += 2) { $named[$gameArgs[$i].TrimStart('-')] = $gameArgs[$i + 1] }
        if ($restoreRemote) { $named['RestoreFrom'] = $restoreRemote }
        if ($OwnerName) { $named['OwnerName'] = $OwnerName }
        if ($ServerName) { $named['ServerName'] = $ServerName }
        if ($Mode -eq 'Public') { $named['PublicHost'] = $PublicHost.ToLowerInvariant(); $named['GatewayPort'] = [int]$GatewayPort }
        if ($AdminIp) { $named['AdminIp'] = @($AdminIp) }
        if ($KeepRdpOpen) { $switches += 'KeepRdpOpen' }
        if ($InteractiveSession) { $switches += 'InteractiveSession' }
        if ($Chat) { $named['Chat'] = $Chat }
    }
    $cmdParts = @("& $(ConvertTo-DRPsLiteral (Join-Path $RemoteKit $scriptName))")
    foreach ($k in $named.Keys) {
        $v = $named[$k]
        if ($v -is [int]) { $cmdParts += "-$k $v" }
        elseif ($v -is [array]) { $cmdParts += "-$k " + (($v | ForEach-Object { ConvertTo-DRPsLiteral ([string]$_) }) -join ',') }
        else { $cmdParts += "-$k " + (ConvertTo-DRPsLiteral ([string]$v)) }
    }
    foreach ($s in $switches) { $cmdParts += "-$s" }
    # 6>&1: over Windows OpenSSH, Write-Host output reaches this PC twice (stdout and the console);
    # sending the host stream into the output stream makes each line arrive once.
    $remoteCmd = ($cmdParts -join ' ') + ' 6>&1; exit $LASTEXITCODE'
    if ($UploadOnly -or $script:Test) {
        Write-DRStep 'Ready'
        Write-DRInfo "everything is on the server. The command that would run there$(if ($script:Test) { ' (not run in a test)' }):"
        Write-Host "   $remoteCmd"
        exit 0
    }

    Write-DRStep "Running $scriptName on the server (its output follows)"
    $out = Invoke-Remote $remoteCmd -Stream
    $installExit = $script:RemoteExit
    if ($installExit -ne 0) { Stop-DR "$scriptName failed on the server (exit code $installExit). Fix what it says and run this command again: finished steps are kept." }
    if ($restoreRemote) {
        [void](Invoke-Remote "Remove-Item -LiteralPath $(ConvertTo-DRPsLiteral $restoreRemote) -Recurse -Force; 'DRJSON:{}'")
        Write-DROk 'the uploaded backup copy was deleted from the staging folder (the install keeps what it needs)'
    }

    # --- result ---------------------------------------------------------------------------------
    Write-DRStep 'Result'
    $cj = Get-RemoteJson (Invoke-Remote "`$c = Get-Content -LiteralPath $(ConvertTo-DRPsLiteral (Join-Path $InstallRoot 'data\config\server.json')) -Raw | ConvertFrom-Json; 'DRJSON:' + (@{ mode = `$c.Mode; host = `$c.PublicHost; port = `$c.Ports.gateway; fp = `$c.CertFingerprint; name = `$c.ServerName; commit = `$c.Commit } | ConvertTo-Json -Compress)")
    if (-not $cj) { Stop-DR 'Could not read server.json on the server.' }
    Write-Host ''
    Write-Host "  Server      : $($cj.name) (code $($cj.commit))" -ForegroundColor Green
    if ($cj.mode -eq 'Public') {
        Write-Host "  Address     : $($cj.host):$($cj.port)" -ForegroundColor Green
        Write-Host "  Fingerprint : $($cj.fp)" -ForegroundColor Green
        Write-Host ''
        Write-DRInfo 'checking from this PC through the public gateway (TLS pinned to that fingerprint) ...'
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'Get-ServerStatus.ps1') -Server "$($cj.host):$($cj.port)" -Fingerprint $cj.fp
        if ($LASTEXITCODE -ne 0) { Write-DRWarn "this PC could not reach the gateway. Check the provider's firewall (TCP $($cj.port) must be open) and run Stack.ps1 status on the server." }
        Write-Host ''
        Write-Host '  At your provider (cloud firewall / security group), allow inbound from ANY address:' -ForegroundColor Yellow
        Write-Host "     TCP $($cj.port)  (the gateway)   and   UDP 8770-8777  (the game servers)" -ForegroundColor Yellow
        Write-Host '  This server''s own allowlist limits UDP to logged-in players; this PC cannot test UDP.' -ForegroundColor Yellow
        Write-Host '  Without the UDP rule friends log in and then hang loading Ramsgate. Also limit TCP 22 and' -ForegroundColor Yellow
        Write-Host '  3389 to your own address at the provider.' -ForegroundColor Yellow
        Write-Host ''
        Write-Host "  Invites     : .\Deploy-Remote.ps1 -Server $Server -InviteFor <name>" -ForegroundColor Cyan
    }
} catch {
    if ("$_" -notmatch '^DRFAIL:') { Write-Host "   FAIL  $_" -ForegroundColor Red; Write-Host "         at $($_.InvocationInfo.PositionMessage)" }
    $exitCode = 1
}
exit $exitCode
