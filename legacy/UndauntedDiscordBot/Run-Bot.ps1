param(
    [string]$Root = 'C:\DauntlessRevived',
    [string]$Node = 'C:\Program Files\nodejs\node.exe'
)
$ErrorActionPreference = 'Stop'
$app = Join-Path $Root 'discord-keys'
$envFile = Join-Path $Root 'data/config/discord-keys.env'
$logs = Join-Path $Root 'data/logs'
if (!(Test-Path -LiteralPath $envFile)) { throw 'Missing protected discord-keys.env' }
New-Item -ItemType Directory -Path $logs -Force | Out-Null
$delay = 5
while ($true) {
    $started = Get-Date
    try {
        $child = Start-Process -FilePath $Node -WorkingDirectory $app -WindowStyle Hidden -PassThru -Wait `
            -ArgumentList @("--env-file=`"$envFile`"", 'bot.mjs') `
            -RedirectStandardOutput (Join-Path $logs 'discord-keys.out.log') `
            -RedirectStandardError (Join-Path $logs 'discord-keys.err.log')
        $result = "Bot exited with code $($child.ExitCode)"
    } catch { $result = 'Bot could not start; check runtime and protected environment file' }
    if (((Get-Date) - $started).TotalSeconds -ge 60) { $delay = 5 }
    Add-Content -LiteralPath (Join-Path $logs 'discord-keys-supervisor.log') -Value "$(Get-Date -Format o) $result; retry in ${delay}s"
    Start-Sleep -Seconds $delay
    $delay = [Math]::Min(60, $delay * 2)
}
