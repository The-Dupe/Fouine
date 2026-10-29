# Verifie l'arriere-plan : fermeture vers la zone de notification, surveillance + sauvegarde fenetre cachee,
# reaffichage par une seconde instance, instance unique, mode --daemon.
# Pas de SEE_MASK_NOZONECHECKS : l'exe doit etre signe (tools\signature\signer.ps1) pour ne pas declencher d'avertissement.
Add-Type -AssemblyName Microsoft.VisualBasic
Add-Type @"
using System; using System.Collections.Generic; using System.Runtime.InteropServices;
public class V {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L, T, R, B; }
  // Vrai si un des processus donnes possede une fenetre de haut niveau visible d'au moins 200x100.
  public static bool GrandeFenetreVisible(uint[] pids) {
    bool trouve = false;
    EnumWindows((h, l) => {
      uint pid; GetWindowThreadProcessId(h, out pid);
      RECT r;
      if (Array.IndexOf(pids, pid) >= 0 && IsWindowVisible(h) && GetWindowRect(h, out r) && r.R - r.L >= 200 && r.B - r.T >= 100) trouve = true;
      return true;
    }, IntPtr.Zero);
    return trouve;
  }
}
"@
Get-Process fouine -ErrorAction SilentlyContinue | Stop-Process -Force
$e = Split-Path $PSScriptRoot
$h = "$e\target\ui-fond"; Remove-Item $h -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $h | Out-Null
$d = "$e\target\ui-fond-data"; Remove-Item $d -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $d | Out-Null
"a.txt", "b.txt" | ForEach-Object { Set-Content "$d\$_" "x" }
(@{ dossiers = @($d); exclusions = @(); theme = "sombre" } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$env:FOUINE_HOME = $h; $env:FOUINE_LOG = "$h\log.txt"; $env:FOUINE_SAVE_SECS = "2"
function Attendre($cond, $s = 30) { $t = Get-Date; while (((Get-Date) - $t).TotalSeconds -lt $s) { if (& $cond) { return $true }; Start-Sleep -Milliseconds 250 }; return $false }
function Visible { $ids = [uint32[]]@(Get-Process fouine -ErrorAction SilentlyContinue | ForEach-Object { $_.Id }); return [V]::GrandeFenetreVisible($ids) }

$p = Start-Process "$e\target\release\fouine.exe" -PassThru
"demarre : " + (Attendre { (Test-Path "$h\log.txt") -and (Select-String -Path "$h\log.txt" -Pattern 'surveillance de' -Quiet) })
"droits/service : " + ((Get-Content "$h\log.txt" | Select-String 'droits:').Line)
"fenetre visible au depart : " + (Visible)

[void]$p.CloseMainWindow(); Start-Sleep 3
$p.Refresh()
"apres fermeture : processus vivant = " + (-not $p.HasExited) + " ; fenetre visible = " + (Visible)
(Get-Content "$h\log.txt" | Select-String 'zone de notification').Line

$m0 = (Get-Item "$h\index.db" -ErrorAction SilentlyContinue).LastWriteTime
Set-Content "$d\nouveau-fichier.txt" "y"
Remove-Item "$d\a.txt"
$ok = Attendre { $m = (Get-Item "$h\index.db" -ErrorAction SilentlyContinue).LastWriteTime; $m -and $m -ne $m0 } 15
"index enregistre fenetre cachee apres ajout/suppression : $ok"

$p2 = Start-Process "$e\target\release\fouine.exe" -PassThru
Start-Sleep 3
"seconde instance terminee : " + $p2.HasExited
"fenetre reaffichee par la seconde instance : " + (Attendre { Visible } 8)
"instances fouine : " + @(Get-Process fouine -ErrorAction SilentlyContinue).Count
$p.Kill(); Start-Sleep 1

# Mode sans fenetre (celui du service) : indexe, surveille, enregistre
Remove-Item $h -Recurse -Force; New-Item -ItemType Directory $h | Out-Null
(@{ dossiers = @($d); exclusions = @(); theme = "sombre" } | ConvertTo-Json -Compress) | Set-Content "$h\reglages.json" -Encoding ascii
$q = Start-Process "$e\target\release\fouine.exe" -ArgumentList "--daemon", "25" -PassThru
Attendre { Test-Path "$h\index.db" } 15 | Out-Null
Start-Sleep 2
$m1 = (Get-Item "$h\index.db").LastWriteTime
Set-Content "$d\pendant-daemon.txt" "z"
"daemon : base mise a jour apres ajout = " + (Attendre { (Get-Item "$h\index.db").LastWriteTime -ne $m1 } 15)
$q.WaitForExit(30000) | Out-Null
"daemon termine proprement : " + $q.HasExited
Get-Content "$h\log.txt" | Select-String 'indexation|surveillance|arret' | ForEach-Object { $_.Line }
