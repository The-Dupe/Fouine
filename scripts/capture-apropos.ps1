# Ouvre le menu principal puis la fenetre « A propos » et enregistre des captures (target\ui-apropos-*.png).
# Usage : capture-apropos.ps1 [chemin de fouine.exe]  (par defaut : target\release\fouine.exe)
param([string]$Exe = '')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class A {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public struct RECT { public int L, T, R, B; }
  public static void Click(int x, int y) { SetCursorPos(x, y); System.Threading.Thread.Sleep(150); mouse_event(2, 0, 0, 0, UIntPtr.Zero); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
}
"@
Get-Process fouine* -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 3   # laisse le systeme liberer le verrou d'instance unique de l'ancienne instance
$e = Split-Path $PSScriptRoot
if (-not $Exe) { $Exe = "$e\target\release\fouine.exe" }
$h = "$e\target\ui-apropos"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
(@{ dossiers = @("$e\scripts"); exclusions = @(); theme = "sombre"; zone_notification = $false } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process $Exe -PassThru
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) { if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }; Start-Sleep -Milliseconds 200 }
Start-Sleep 2
$p.Refresh()
while ($p.MainWindowHandle -eq 0 -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 300; $p.Refresh() }
if ($p.MainWindowHandle -eq 0) { "fenetre introuvable : aucun clic envoye"; $p.Kill(); exit 1 }
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
$r = New-Object A+RECT
function Rect { [void][A]::GetWindowRect($p.MainWindowHandle, [ref]$script:r) }
function Shot($nom) { Rect; $b = New-Object System.Drawing.Bitmap ($script:r.R - $script:r.L), ($script:r.B - $script:r.T); [System.Drawing.Graphics]::FromImage($b).CopyFromScreen($script:r.L, $script:r.T, 0, 0, $b.Size); $b.Save("$e\target\ui-apropos-$nom.png", [System.Drawing.Imaging.ImageFormat]::Png) }
function ClickAt($dx, $dy) { Rect; [A]::Click($script:r.L + $dx, $script:r.T + $dy); Start-Sleep -Milliseconds 800 }
ClickAt 1075 65; Shot "1-menu"
ClickAt 905 498; Shot "2-apropos"
[System.Windows.Forms.SendKeys]::SendWait("{ESC}"); Start-Sleep -Milliseconds 500
# Quitter par Ctrl+Q : le processus doit se terminer rapidement
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep -Milliseconds 500
$t1 = Get-Date
[System.Windows.Forms.SendKeys]::SendWait("^q")
[void]$p.WaitForExit(15000)
"Ctrl+Q : processus termine = {0} en {1:N1} s" -f $p.HasExited, ((Get-Date) - $t1).TotalSeconds
if (-not $p.HasExited) { $p.Kill() }
