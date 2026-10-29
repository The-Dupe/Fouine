# Test de fumee de l'application (surveillance en direct) : attendu `sortie: 25 entrees` dans le journal.
# Prerequis : cargo build --release -p fouine-app
Get-Process fouine -ErrorAction SilentlyContinue | Stop-Process -Force   # ecarte les instances orphelines d'un essai precedent
$e = Split-Path $PSScriptRoot
$h = "$e\target\smoke"
Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
$d = "$e\target\smoke-data"
Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory "$d\sub" | Out-Null
1..20 | ForEach-Object { Set-Content "$d\sub\f$_.txt" "x" }          # 22 entrees avec la racine et sub
(@{ dossiers = @($d); exclusions = @(); zone_notification = $false } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process "$e\target\release\fouine.exe" -PassThru
$t0 = Get-Date   # attend que la surveillance soit active (jusqu'a 40 s) avant de modifier le disque
while (((Get-Date) - $t0).TotalSeconds -lt 40) {
    if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }
    Start-Sleep -Milliseconds 200
}
Set-Content "$d\sub\vivant.txt" "x"; Set-Content "$d\sub\vivant2.txt" "x"      # +2
New-Item -ItemType Directory "$d\neuf" | Out-Null; Set-Content "$d\neuf\vivant3.txt" "x"   # +2
Start-Sleep 2
Remove-Item "$d\sub\f1.txt"; Start-Sleep 2                                    # -1
$p.CloseMainWindow() | Out-Null; [void]$p.WaitForExit(15000)
if (-not $p.HasExited) { $p.Kill(); "processus tue (fermeture non propre)" }
Get-Content "$h\log.txt"
