# Mesure le delai de demarrage de l'application sur plusieurs lancements (diagnostic du blocage intermittent).
# Usage : powershell -File scripts\demarrages.ps1 [nombre]   (prerequis : cargo build --release -p fouine-app)
param([int]$N = 6)
$e = Split-Path $PSScriptRoot
$d = "$e\target\smoke-data"; New-Item -ItemType Directory -Force "$d\sub" | Out-Null
for ($i = 1; $i -le $N; $i++) {
    $h = "$e\target\demarrage-$i"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
    (@{ dossiers = @($d); exclusions = @() } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
    $env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
    $t0 = Get-Date
    $p = Start-Process "$e\target\release\fouine.exe" -PassThru
    $vu = $null
    while (((Get-Date) - $t0).TotalSeconds -lt 20) {
        if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'premiere image' -Quiet)) { $vu = ((Get-Date) - $t0).TotalSeconds; break }
        Start-Sleep -Milliseconds 100
    }
    $etat = if ($vu) { "premiere image en {0:N1} s" -f $vu } else { "AUCUNE IMAGE en 20 s (processus vivant: $(-not $p.HasExited), fenetre: '$($p.MainWindowTitle)')" }
    "{0}: {1}" -f $i, $etat
    if ($p.HasExited) { "   le processus s'est termine seul (code $($p.ExitCode))" } else { $p.CloseMainWindow() | Out-Null; Start-Sleep 2; if (-not $p.HasExited) { $p.Kill() } }
    Start-Sleep 1
}
