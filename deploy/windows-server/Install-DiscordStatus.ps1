<# Private webhook secret must already exist at data\keys\discord-status.key.
   Installs only a LocalService monitor. Does not restart the game stack. #>
[CmdletBinding()]
param([string]$Root)
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\DauntlessServer.Common.ps1"
$Root = Resolve-DRRoot $Root $PSScriptRoot
$P = Get-DRPaths $Root
$Cfg = Get-DRConfig $Root
if (-not (Test-DRAdmin)) { throw 'Run as Administrator.' }
$dir = Join-Path $Root 'discord-status'
$state = Join-Path $dir 'state'
$secret = Join-Path $P.Keys 'discord-status.key'
foreach ($path in @($Root, (Join-Path $Root 'data'), $P.Keys, $secret, $P.OwnerKey, $dir, $state, (Join-Path $dir 'worker.ts'), (Join-Path $dir 'worker.mjs'), (Join-Path $dir 'worker.env'))) {
    if (Test-DRReparsePoint $path) { throw 'Refusing linked status-worker path.' }
}
if (-not (Test-Path -LiteralPath $secret)) { throw 'Create the private discord-status.key file first.' }
$task = 'Dauntless Revived Discord status'
if (Get-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue) { Stop-ScheduledTask -TaskName $task }
if (-not (Test-Path -LiteralPath (Join-Path $P.App 'tools\discord-status.ts'))) { throw 'Update the server code before installing the status worker.' }
$bun = Install-DRBun $P
New-Item -ItemType Directory -Force -Path $dir | Out-Null
icacls.exe $dir /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F' '*S-1-5-19:(OI)(CI)RX' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Worker ACL failed' }
New-Item -ItemType Directory -Force -Path $state | Out-Null
icacls.exe $state /grant:r '*S-1-5-19:(OI)(CI)M' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'State ACL failed' }
foreach ($path in @($Root, (Join-Path $Root 'data'), $P.Keys)) {
    icacls.exe $path /grant '*S-1-5-19:RX' | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Parent ACL failed' }
}
icacls.exe $secret /inheritance:r /grant:r '*S-1-5-32-544:F' '*S-1-5-18:F' '*S-1-5-19:R' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Secret ACL failed' }
icacls.exe $P.OwnerKey /grant '*S-1-5-19:R' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Owner key ACL failed' }
Copy-Item -LiteralPath (Join-Path $P.App 'tools\discord-status.ts') -Destination (Join-Path $dir 'worker.ts') -Force
# The Node version's copy
if (Test-Path -LiteralPath (Join-Path $dir 'worker.mjs')) { Remove-DRItemNoFollow (Join-Path $dir 'worker.mjs') }
$envMap = [ordered]@{
    STATUS_OWNER_KEY_FILE = $P.OwnerKey.Replace('\', '/')
    STATUS_WEBHOOK_FILE = $secret.Replace('\', '/')
    STATUS_STATE_FILE = (Join-Path $state 'message.json').Replace('\', '/')
    STATUS_ERROR_FILE = (Join-Path $state 'errors.log').Replace('\', '/')
    STATUS_BACKEND = "http://127.0.0.1:$(Get-DRPort $Cfg 'metagame')"
}
[void](Write-DREnv (Join-Path $dir 'worker.env') $envMap @('Paths only. Never put the webhook token in source control.'))
# Only the named env file: Bun would otherwise also read a .env in the working folder
$action = New-ScheduledTaskAction -Execute $bun -Argument "--no-env-file --env-file=`"$dir\worker.env`" `"$dir\worker.ts`"" -WorkingDirectory $dir
$principal = New-ScheduledTaskPrincipal -UserId 'S-1-5-19' -LogonType ServiceAccount
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew -StartWhenAvailable -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
Register-ScheduledTask -TaskName $task -Action $action -Principal $principal -Trigger (New-ScheduledTaskTrigger -AtStartup) -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $task
Write-Host 'Discord status worker installed. No game processes restarted.'
