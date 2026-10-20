# Builds the friend kit zip from friend-kit\ plus the two pinned DLLs, the licence and a SOURCE.txt
# naming the exact commit (AGPL: players of a modified server are owed its source).
#   powershell -ExecutionPolicy Bypass -File tools\make-friend-kit.ps1 [-Out C:\dr\dist]
param([string]$Out = "C:\dr\dist")
$ErrorActionPreference = "Stop"

$Repo = Split-Path $PSScriptRoot -Parent
$Pinned = @{
  "dxgi.dll"                    = "9A431D7B6FD20C43FA92BEBD91C3BC023EC7A3FCBC52871C41F4DF293D4B0D1F"
  "UndauntedInternalServer.dll" = "6f2433abebdee451edfc9060fa58eb3e2a3e73e5f7063de05386316b52d7b30b"
}

$commit = (git -C $Repo rev-parse HEAD).Trim()
$dirty = [bool](git -C $Repo status --porcelain -- friend-kit client-mods)
if ($dirty) { throw "friend-kit\ or client-mods\ has uncommitted changes. Commit and push first, so SOURCE.txt names real code." }
$remote = (git -C $Repo remote get-url origin).Trim() -replace '\.git$', ''

$Stage = Join-Path $Out "DauntlessRevived-FriendKit"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force (Join-Path $Stage "dll") | Out-Null

foreach ($f in "setup.ps1", "play.ps1", "Setup.cmd", "Play Dauntless.cmd", "README.txt", "THIRD-PARTY-NOTICES.txt") {
  Copy-Item -LiteralPath (Join-Path $Repo "friend-kit\$f") -Destination $Stage
}
foreach ($name in $Pinned.Keys) {
  $src = Join-Path $Repo "legacy\UndauntedLauncher\assets\$name"
  if ((Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash -ne $Pinned[$name]) { throw "$name does not match its pinned hash" }
  Copy-Item -LiteralPath $src -Destination (Join-Path $Stage "dll")
}
Copy-Item -LiteralPath (Join-Path $Repo "LICENSE.txt") -Destination $Stage
# UE4SS and the client mods; setup.ps1 installs them (checked against client-mods\manifest.json).
Copy-Item -LiteralPath (Join-Path $Repo "client-mods") -Destination (Join-Path $Stage "client-mods") -Recurse
@(
  "Dauntless Revived - source code (AGPL-3.0)",
  "",
  "Repository: $remote",
  "Version:    $commit",
  "Browse it:  $remote/tree/$commit",
  "",
  "Based on Undaunted (https://github.com/SyST3MDeV/Undaunted), AGPL-3.0."
) | Set-Content -LiteralPath (Join-Path $Stage "SOURCE.txt") -Encoding ASCII

Get-ChildItem -LiteralPath $Stage -Recurse -File | Sort-Object FullName | ForEach-Object {
  "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLower())  $($_.FullName.Substring($Stage.Length + 1).Replace('\', '/'))"
} | Set-Content -LiteralPath (Join-Path $Stage "SHA256SUMS.txt") -Encoding ASCII

$zip = Join-Path $Out "DauntlessRevived-FriendKit-$($commit.Substring(0, 7)).zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $zip
"kit:    $zip"
"sha256: $((Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash)"
