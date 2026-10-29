# Verifie a l'ecran (clics et touches reels) : tri par clic sur l'en-tete, Entree, Ctrl+Entree, double-clic.
# Preuve : lignes "tri:", "ouvrir:", "afficher:" du journal + processus lances. Prerequis : cargo build --release -p fouine-app.
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public struct RECT { public int L, T, R, B; }
  public static void Click(int x, int y, int n) { SetCursorPos(x, y); System.Threading.Thread.Sleep(120);
    for (int i = 0; i < n; i++) { mouse_event(0x2, 0, 0, 0, UIntPtr.Zero); mouse_event(0x4, 0, 0, 0, UIntPtr.Zero); System.Threading.Thread.Sleep(60); } }
}
"@
Get-Process fouine -ErrorAction SilentlyContinue | Stop-Process -Force
$e = Split-Path $PSScriptRoot
$h = "$e\target\ui-verif"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
$d = "$e\target\ui-data"; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory "$d\Documents", "$d\Photos" | Out-Null
"rapport-annuel.pdf", "rapport-final.docx", "budget-2024.xlsx", "notes.txt" | ForEach-Object { Set-Content "$d\Documents\$_" ("x" * 4096) }
"plage.jpg", "ete-2024.png", "f1-monaco.jpg" | ForEach-Object { Set-Content "$d\Photos\$_" ("x" * 200000) }
(@{ dossiers = @($d); exclusions = @(); theme = "sombre" } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:SEE_MASK_NOZONECHECKS = "1"; $env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
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
    $bmp = New-Object System.Drawing.Bitmap ($script:r.R - $script:r.L), ($script:r.B - $script:r.T)
    [System.Drawing.Graphics]::FromImage($bmp).CopyFromScreen($script:r.L, $script:r.T, 0, 0, $bmp.Size)
    $bmp.Save("$e\target\ui-verif-$nom.png", [System.Drawing.Imaging.ImageFormat]::Png)
}
function ClickAt($dx, $dy, $n = 1) { Rect; [W]::Click($script:r.L + $dx, $script:r.T + $dy, $n); Start-Sleep -Milliseconds 800 }
function Journal { Get-Content "$h\log.txt" | Select-String 'tri:|ouvrir:|afficher:' | ForEach-Object { $_.Line } }

$avant = @(Get-Process notepad, explorer -ErrorAction SilentlyContinue).Count
[System.Windows.Forms.SendKeys]::SendWait("notes.txt"); Start-Sleep 1
Shot "1-recherche"
[System.Windows.Forms.SendKeys]::SendWait("{DOWN}{ENTER}"); Start-Sleep 2
"--- apres Entree"; Journal
Get-Process notepad -ErrorAction SilentlyContinue | Stop-Process -Force
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
Get-Process notepad -ErrorAction SilentlyContinue | Select-Object Id, MainWindowTitle | Format-Table | Out-String
[System.Windows.Forms.SendKeys]::SendWait("^{ENTER}"); Start-Sleep 2
"--- apres Ctrl+Entree"; Journal
Shot "2-apres-ouverture"
# tri : efface la recherche, clique les en-tetes Taille puis Taille (inverse) puis Nom
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
[System.Windows.Forms.SendKeys]::SendWait("^a{BACKSPACE}2024"); Start-Sleep 1
ClickAt 958 164; Shot "3-tri-taille"
ClickAt 958 164; Shot "4-tri-taille-inverse"
ClickAt 150 164; Shot "5-tri-nom"
ClickAt 300 210 2                    # double-clic sur le premier resultat
"--- journal final"; Journal
$p.CloseMainWindow() | Out-Null; Start-Sleep 2; if (-not $p.HasExited) { $p.Kill() }
