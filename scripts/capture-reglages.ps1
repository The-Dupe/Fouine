# Ouvre la fenetre « Volumes et dossiers indexes » (menu, puis « Dossiers indexes... ») et enregistre une capture.
# Sortie : target\ui-reglages-*.png. Usage : capture-reglages.ps1 [chemin de fouine.exe]
param([string]$Exe = '')
Add-Type -AssemblyName System.Windows.Forms, System.Drawing, Microsoft.VisualBasic
Add-Type @"
using System; using System.Runtime.InteropServices;
public class R {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  public struct RECT { public int L, T, R, B; }
  public static void Click(int x, int y) { SetCursorPos(x, y); System.Threading.Thread.Sleep(150); mouse_event(2, 0, 0, 0, UIntPtr.Zero); mouse_event(4, 0, 0, 0, UIntPtr.Zero); }
}
"@
Get-Process fouine* -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep 3
$e = Split-Path $PSScriptRoot
if (-not $Exe) { $Exe = "$e\target\release\fouine.exe" }
$h = "$e\target\ui-reglages"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
(@{ dossiers = @("$e\scripts"); theme = "sombre"; zone_notification = $false } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"
$p = Start-Process $Exe -PassThru
$t0 = Get-Date
while (((Get-Date) - $t0).TotalSeconds -lt 40) { if ((Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet)) { break }; Start-Sleep -Milliseconds 200 }
Start-Sleep 2; $p.Refresh()
while ($p.MainWindowHandle -eq 0 -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Milliseconds 300; $p.Refresh() }
if ($p.MainWindowHandle -eq 0) { "fenetre introuvable"; $p.Kill(); exit 1 }
[Microsoft.VisualBasic.Interaction]::AppActivate($p.Id); Start-Sleep 1
$r = New-Object R+RECT
function Rect { [void][R]::GetWindowRect($p.MainWindowHandle, [ref]$script:r) }
function Shot($nom) { Rect; $b = New-Object System.Drawing.Bitmap ($script:r.R - $script:r.L), ($script:r.B - $script:r.T); [System.Drawing.Graphics]::FromImage($b).CopyFromScreen($script:r.L, $script:r.T, 0, 0, $b.Size); $b.Save("$e\target\ui-reglages-$nom.png", [System.Drawing.Imaging.ImageFormat]::Png) }
function ClickAt($dx, $dy) { Rect; [void][R]::SetCursorPos($script:r.L + $dx, $script:r.T + $dy); Start-Sleep -Milliseconds 400; [R]::Click($script:r.L + $dx, $script:r.T + $dy); Start-Sleep -Milliseconds 900 }
ClickAt 1075 65     # menu principal
ClickAt 900 140     # « Dossiers indexes... »
Shot "1-fenetre"
$p.CloseMainWindow() | Out-Null; [void]$p.WaitForExit(15000); if (-not $p.HasExited) { $p.Kill() }
