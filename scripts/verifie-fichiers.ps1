# Verifie le menu contextuel des fichiers (couper, copier, coller, ouvrir le dossier, supprimer) avec de vrais clics.
# Jeu de test : target\fichiers-test\ (Source\note-alpha.txt, Source\note-beta.txt, Cible\). Captures : target\ui-fichiers-*.png
# Usage : verifie-fichiers.ps1 [-Exe chemin\fouine.exe]
param([string]$Exe = '')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class F {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public struct RECT { public int L, T, R, B; }
  public static void Click(int x, int y, bool right) { SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
    uint down = right ? 0x8u : 0x2u, up = right ? 0x10u : 0x4u; mouse_event(down, 0, 0, 0, UIntPtr.Zero); mouse_event(up, 0, 0, 0, UIntPtr.Zero); }
}
"@
Get-Process fouine* -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 3
$e = Split-Path $PSScriptRoot
if (-not $Exe) { $Exe = "$e\target\release\fouine.exe" }
$d = "$e\target\fichiers-test"; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory "$d\Source", "$d\Cible" | Out-Null
Set-Content "$d\Source\note-alpha.txt" "contenu de test"
Set-Content "$d\Source\note-beta.txt" "second fichier"
$h = "$e\target\ui-fichiers"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
(@{ dossiers = @($d); exclusions = @(); theme = "sombre"; zone_notification = $false } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process $Exe -PassThru
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) { if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }; Start-Sleep -Milliseconds 200 }
Start-Sleep 2; $p.Refresh()
while ($p.MainWindowHandle -eq 0 -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 300; $p.Refresh() }
if ($p.MainWindowHandle -eq 0) { "fenetre introuvable"; $p.Kill(); exit 1 }
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
$r = New-Object F+RECT
function Rect { [void][F]::GetWindowRect($p.MainWindowHandle, [ref]$script:r) }
function Shot($nom) { Rect; $b = New-Object System.Drawing.Bitmap ($script:r.R - $script:r.L), ($script:r.B - $script:r.T); [System.Drawing.Graphics]::FromImage($b).CopyFromScreen($script:r.L, $script:r.T, 0, 0, $b.Size); $b.Save("$e\target\ui-fichiers-$nom.png", [System.Drawing.Imaging.ImageFormat]::Png) }
function ClickAt($dx, $dy, $right = $false) { Rect; [F]::Click($script:r.L + $dx, $script:r.T + $dy, $right); Start-Sleep -Milliseconds 900 }
function Cherche($txt) { [System.Windows.Forms.SendKeys]::SendWait("^f"); Start-Sleep -Milliseconds 300; [System.Windows.Forms.SendKeys]::SendWait("^a" + $txt); Start-Sleep -Milliseconds 1200 }
# Clic droit sur le premier resultat puis clic sur l'entree situee $dy pixels plus bas
# (fichier : Ouvrir le dossier = 58, Couper = 138, Copier = 172, Supprimer = 298 ; dossier : Coller = 206)
function Menu($dy) { Rect; [F]::SetCursorPos($script:r.L + 300, $script:r.T + 215); Start-Sleep -Milliseconds 500; ClickAt 300 210 $true; ClickAt 340 (210 + $dy) }
function Ok($nom, $cond) { "{0,-52} {1}" -f $nom, $(if ($cond) { 'OK' } else { 'ECHEC' }) }

# 1. Copier (fichier) puis Coller dans le dossier Cible
Cherche "note-alpha"
Menu 172
$cb = Get-Clipboard -Format FileDropList
Ok "Copier : le fichier est dans le presse-papiers" (@($cb | Where-Object { $_ -like '*note-alpha.txt' }).Count -eq 1)
Cherche "cible"
Menu 206
Start-Sleep 3
Ok "Coller : copie dans Cible, original conserve" ((Test-Path "$d\Cible\note-alpha.txt") -and (Test-Path "$d\Source\note-alpha.txt"))
Shot "2-apres-collage"

# 2. Couper puis Coller
Cherche "note-beta"
Menu 138
Cherche "cible"
Menu 206
Start-Sleep 3
Ok "Couper+Coller : deplace dans Cible, retire de Source" ((Test-Path "$d\Cible\note-beta.txt") -and -not (Test-Path "$d\Source\note-beta.txt"))

# 3. Ouvrir le dossier : une fenetre de l'Explorateur doit s'ouvrir sur le dossier du fichier
$sh = New-Object -ComObject Shell.Application
Cherche "note-beta"
Menu 58
Start-Sleep 3
$ouvertes = @($sh.Windows() | Where-Object { $_.LocationURL -like '*fichiers-test/Cible*' })
Ok "Ouvrir le dossier : fenetre de l'Explorateur sur Cible" ($ouvertes.Count -ge 1)
$ouvertes | ForEach-Object { $_.Quit() }

# 4. Supprimer (corbeille) : Windows demande confirmation, on valide par Entree
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep -Milliseconds 800
Cherche "note-beta"
Menu 298
Start-Sleep -Milliseconds 1500
[System.Windows.Forms.SendKeys]::SendWait("{ENTER}"); Start-Sleep 3
Shot "3-apres-suppression"
Ok "Supprimer : fichier retire du disque (corbeille)" (-not (Test-Path "$d\Cible\note-beta.txt"))
$p.CloseMainWindow() | Out-Null; [void]$p.WaitForExit(15000); if (-not $p.HasExited) { $p.Kill() }
