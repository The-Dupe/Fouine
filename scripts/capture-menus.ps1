# Exerce les menus et interactions a l'ecran (clics reels) et enregistre une capture apres chaque action.
# Sorties : target\ui-menu-<etape>.png. Prerequis : cargo build --release -p fouine-app.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public struct RECT { public int L, T, R, B; }
  public static void Click(int x, int y, bool right) { SetCursorPos(x, y); System.Threading.Thread.Sleep(120);
    uint down = right ? 0x8u : 0x2u, up = right ? 0x10u : 0x4u; mouse_event(down, 0, 0, 0, UIntPtr.Zero); mouse_event(up, 0, 0, 0, UIntPtr.Zero); }
}
"@
Get-Process fouine -ErrorAction SilentlyContinue | Stop-Process -Force
$e = Split-Path $PSScriptRoot
$h = "$e\target\ui"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
$d = "$e\target\ui-data"; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory "$d\Documents", "$d\Photos" | Out-Null
"rapport-annuel.pdf", "rapport-final.docx", "budget-2024.xlsx", "notes.txt" | ForEach-Object { Set-Content "$d\Documents\$_" ("x" * 4096) }
"plage.jpg", "ete-2024.png", "f1-monaco.jpg" | ForEach-Object { Set-Content "$d\Photos\$_" ("x" * 200000) }
(@{ dossiers = @($d); exclusions = @(); theme = "sombre" } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
'{"history":["rapport ext:pdf","type:image 2024"],"bookmarks":{"PDF recents":"ext:pdf dm:thisyear"}}' | Set-Content "$h\favoris.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process "$e\target\release\fouine.exe" -PassThru
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) {
    if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }
    Start-Sleep -Milliseconds 200
}
Start-Sleep 2; $p.Refresh()
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
$r = New-Object W+RECT
function Rect { [void][W]::GetWindowRect($p.MainWindowHandle, [ref]$script:r) }
function Shot($nom) {
    Rect
    $w = $script:r.R - $script:r.L; $hh = $script:r.B - $script:r.T
    $bmp = New-Object System.Drawing.Bitmap $w, $hh
    [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($script:r.L, $script:r.T, 0, 0, $bmp.Size)
    $bmp.Save("$e\target\ui-menu-$nom.png", [System.Drawing.Imaging.ImageFormat]::Png)
    "capture $nom"
}
function ClickAt($dx, $dy, $right = $false) { Rect; [W]::Click($script:r.L + $dx, $script:r.T + $dy, $right); Start-Sleep -Milliseconds 700 }

[System.Windows.Forms.SendKeys]::SendWait("rapport"); Start-Sleep 1
ClickAt 1031 65                      # historique avec une recherche en cours
Shot "0-historique-avec-recherche"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 500
Shot "0b-apres-echap"
ClickAt 1075 65                      # menu principal
Shot "1-menu"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 400
ClickAt 1031 65                      # historique
Shot "2-historique"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 400
ClickAt 987 65                       # favoris
Shot "3-favoris"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 400
[System.Windows.Forms.SendKeys]::SendWait("rapport"); Start-Sleep 1
ClickAt 300 210 $true                # clic droit sur le premier resultat
Shot "4-contextuel"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 400
ClickAt 150 130                      # filtre « Documents »
ClickAt 958 164                      # en-tete Taille : tri
[System.Windows.Forms.SendKeys]::SendWait("{DOWN}{DOWN}"); Start-Sleep -Milliseconds 700
Shot "5-filtre-tri-selection"
$p.CloseMainWindow() | Out-Null; Start-Sleep 2; if (-not $p.HasExited) { $p.Kill() }
