# Verifie avec de vrais clics et de vraies touches :
#   1. le clic droit s'ouvre n'importe ou sur une ligne (nom, dossier, taille, date, marges)
#   2. la selection multiple (Ctrl+clic, Maj+clic), Ctrl+A
#   3. les raccourcis Ctrl+C, Ctrl+V (coller dans un dossier) et Suppr (corbeille)
# Jeu de test : target\selection-test\ (Source\tst-1.txt, tst-2.txt, tst-3.txt ; Cible\). Captures : target\ui-selection-*.png
param([string]$Exe = '')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class S {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr e);
  public struct RECT { public int L, T, R, B; }
  public static void Click(int x, int y, bool right) { SetCursorPos(x, y); System.Threading.Thread.Sleep(150);
    uint down = right ? 0x8u : 0x2u, up = right ? 0x10u : 0x4u; mouse_event(down, 0, 0, 0, UIntPtr.Zero); mouse_event(up, 0, 0, 0, UIntPtr.Zero); }
  public static void Key(byte vk, bool down) { keybd_event(vk, 0, down ? 0u : 2u, UIntPtr.Zero); }
}
"@
Get-Process fouine* -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 3
$e = Split-Path $PSScriptRoot
if (-not $Exe) { $Exe = "$e\target\release\fouine.exe" }
$d = "$e\target\selection-test"; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory "$d\Source", "$d\Cible" | Out-Null
1..3 | ForEach-Object { Set-Content "$d\Source\tst-$_.txt" "fichier $_" }
$h = "$e\target\ui-selection"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
(@{ dossiers = @($d); exclusions = @(); theme = "sombre"; zone_notification = $false } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process $Exe -PassThru
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) { if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }; Start-Sleep -Milliseconds 200 }
Start-Sleep 2; $p.Refresh()
while ($p.MainWindowHandle -eq 0 -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 300; $p.Refresh() }
if ($p.MainWindowHandle -eq 0) { "fenetre introuvable"; $p.Kill(); exit 1 }
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
$r = New-Object S+RECT
function Rect { [void][S]::GetWindowRect($p.MainWindowHandle, [ref]$script:r) }
function Shot($nom) { Rect; $b = New-Object System.Drawing.Bitmap ($script:r.R - $script:r.L), ($script:r.B - $script:r.T); [System.Drawing.Graphics]::FromImage($b).CopyFromScreen($script:r.L, $script:r.T, 0, 0, $b.Size); $b.Save("$e\target\ui-selection-$nom.png", [System.Drawing.Imaging.ImageFormat]::Png) }
function Deplace($dx, $dy) { Rect; [void][S]::SetCursorPos($script:r.L + $dx, $script:r.T + $dy); Start-Sleep -Milliseconds 400 }
function ClickAt($dx, $dy, $right = $false) { Deplace $dx $dy; [S]::Click($script:r.L + $dx, $script:r.T + $dy, $right); Start-Sleep -Milliseconds 700 }
function Cherche($txt) { [System.Windows.Forms.SendKeys]::SendWait("^f"); Start-Sleep -Milliseconds 300; [System.Windows.Forms.SendKeys]::SendWait("^a" + $txt); Start-Sleep -Milliseconds 1200 }
function Ok($nom, $cond) { "{0,-58} {1}" -f $nom, $(if ($cond) { 'OK' } else { 'ECHEC' }) }
function NbClicsDroits { @(Get-Content "$h\log.txt" | Select-String 'clic droit').Count }
# lignes : la premiere est a y=210, pas de 34 pixels environ (voir ROW_H)
$y1 = 210; $y2 = 244; $y3 = 278

# 1. Clic droit partout sur la ligne
Cherche "tst-"
foreach ($x in 40, 120, 300, 560, 860, 930, 1040, 1100) {
    $avant = NbClicsDroits
    ClickAt $x $y1 $true
    $ouvert = (NbClicsDroits) -gt $avant
    Ok ("Clic droit a x=$x : menu ouvert") $ouvert
    [System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 400
}

# 2. Selection multiple
ClickAt 300 $y1
[S]::Key(0x11, $true); ClickAt 300 $y3; [S]::Key(0x11, $false)      # Ctrl+clic : lignes 1 et 3
Shot "1-ctrl-clic"
Start-Sleep -Milliseconds 500
# 3. Ctrl+C sur la selection (lignes 1 et 3) puis Ctrl+V dans le dossier Cible
[System.Windows.Forms.SendKeys]::SendWait("^c"); Start-Sleep -Milliseconds 800
$cb = @(Get-Clipboard -Format FileDropList | ForEach-Object { $_ })
Ok "Ctrl+C : 2 fichiers dans le presse-papiers (Ctrl+clic)" ($cb.Count -eq 2)
Cherche "cible"
ClickAt 300 $y1
[System.Windows.Forms.SendKeys]::SendWait("^v"); Start-Sleep 3
Ok "Ctrl+V : les 2 fichiers sont colles dans Cible" ((Test-Path "$d\Cible\tst-1.txt") -and (Test-Path "$d\Cible\tst-3.txt") -and -not (Test-Path "$d\Cible\tst-2.txt"))

# 4. Maj+clic (plage), Ctrl+A puis Suppr
Cherche "tst-"
ClickAt 300 $y1
[S]::Key(0x10, $true); ClickAt 300 $y3; [S]::Key(0x10, $false)       # Maj+clic : plage de 3 lignes
Shot "2-maj-clic"
[System.Windows.Forms.SendKeys]::SendWait("^c"); Start-Sleep -Milliseconds 800
$cb2 = @(Get-Clipboard -Format FileDropList | ForEach-Object { $_ })
Ok "Maj+clic puis Ctrl+C : plage de lignes copiee" ($cb2.Count -ge 3)
[System.Windows.Forms.SendKeys]::SendWait("^a"); Start-Sleep -Milliseconds 500
[System.Windows.Forms.SendKeys]::SendWait("{DELETE}"); Start-Sleep -Milliseconds 1500
[System.Windows.Forms.SendKeys]::SendWait("{ENTER}"); Start-Sleep 3
Shot "3-apres-suppr"
$restants = @(Get-ChildItem "$d" -Recurse -Filter "tst-*.txt").Count
Ok "Ctrl+A puis Suppr : tous les fichiers tst-* envoyes a la corbeille" ($restants -eq 0)
$p.CloseMainWindow() | Out-Null; [void]$p.WaitForExit(15000); if (-not $p.HasExited) { $p.Kill() }
