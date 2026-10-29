# Lance l'application sur un jeu de donnees de test, tape une requete et enregistre une capture d'ecran de la fenetre.
# Prerequis : cargo build --release -p fouine-app. Sortie : target\ui-<nom>.png
param([string]$Requete = "f1", [string]$Nom = "recherche", [string]$Theme = "systeme", [string]$Apres = "")
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W { [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public struct RECT { public int L, T, R, B; } }
"@
Get-Process fouine -ErrorAction SilentlyContinue | Stop-Process -Force
$e = Split-Path $PSScriptRoot
$h = "$e\target\ui"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
$d = "$e\target\ui-data"; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory "$d\Documents", "$d\Photos" | Out-Null
"rapport-annuel.pdf", "rapport-final.docx", "budget-2024.xlsx", "notes.txt" | ForEach-Object { Set-Content "$d\Documents\$_" ("x" * 4096) }
"plage.jpg", "ete-2024.png", "f1-monaco.jpg" | ForEach-Object { Set-Content "$d\Photos\$_" ("x" * 200000) }
(@{ dossiers = @($d); exclusions = @(); theme = $Theme } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process "$e\target\release\fouine.exe" -PassThru
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) {
    if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }
    Start-Sleep -Milliseconds 200
}
Start-Sleep 2
$p.Refresh()
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id)
Start-Sleep 1
if ($Requete) { [System.Windows.Forms.SendKeys]::SendWait($Requete) }
Start-Sleep 1
if ($Apres) { [System.Windows.Forms.SendKeys]::SendWait($Apres); Start-Sleep 1 }
Start-Sleep 1
$r = New-Object W+RECT
[void][W]::GetWindowRect($p.MainWindowHandle, [ref]$r)
$w = $r.R - $r.L; $hh = $r.B - $r.T
"fenetre: ${w}x${hh} a ($($r.L),$($r.T))"
if ($w -gt 100 -and $hh -gt 100) {
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($r.L, $r.T, 0, 0, $bmp.Size)
    $bmp.Save("$e\target\ui-$Nom.png", [System.Drawing.Imaging.ImageFormat]::Png)
    "capture: $e\target\ui-$Nom.png"
}
$p.CloseMainWindow() | Out-Null; Start-Sleep 2; if (-not $p.HasExited) { $p.Kill() }
