# Mesure le temps qu'Everything (installe) met a indexer les volumes jusqu'a avoir son total complet d'objets,
# pour comparer avec Fouine. Lance une instance SEPAREE (-instance fouine-bench) avec sa propre configuration et sa
# propre base dans target\, sur un index vide ; ne touche pas a l'instance habituelle. Session administrateur requise.
# La barre d'etat de la fenetre est lue avec WM_GETTEXT et un delai maximum (SendMessageTimeout) : jamais bloquant,
# contrairement a l'automatisation d'interface qui reste suspendue quand la fenetre est occupee.
# Usage : mesure-everything.ps1 [-Fois 3] [-Minimum 3000000] [-Exe "C:\Program Files\Everything\Everything.exe"]
param([int]$Fois = 3, [int64]$Minimum = 3000000, [string]$Exe = 'C:\Program Files\Everything\Everything.exe')
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public class Barre {
  delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string classe, string titre);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, StringBuilder l, uint flags, uint ms, out IntPtr res);
  public static IntPtr Fenetre(uint pidCherche) {
    IntPtr trouve = IntPtr.Zero;
    EnumWindows((h, l) => { uint pid; GetWindowThreadProcessId(h, out pid); if (pid == pidCherche && FindWindowEx(h, IntPtr.Zero, "msctls_statusbar32", null) != IntPtr.Zero) { trouve = h; return false; } return true; }, IntPtr.Zero);
    return trouve;
  }
  public static string Texte(IntPtr fenetre) {
    IntPtr barre = FindWindowEx(fenetre, IntPtr.Zero, "msctls_statusbar32", null);
    if (barre == IntPtr.Zero) return null;
    var sb = new StringBuilder(512); IntPtr res;
    // WM_GETTEXT = 0x0D ; SMTO_ABORTIFHUNG | SMTO_BLOCK = 0x3 ; delai maximum 150 ms
    if (SendMessageTimeout(barre, 0x0D, (IntPtr)512, sb, 0x3, 150, out res) == IntPtr.Zero) return null;
    return sb.ToString();
  }
}
"@
$e = Split-Path $PSScriptRoot
$dir = "$e\target\everything-bench"
$temps = @()
for ($i = 1; $i -le $Fois; $i++) {
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory $dir | Out-Null
    $t0 = Get-Date
    $p = Start-Process $Exe -ArgumentList '-instance', 'fouine-bench', '-config', "$dir\bench.ini", '-db', "$dir\bench.db" -PassThru
    $etats = [ordered]@{}; $final = $null; $objets = 0; $fen = [IntPtr]::Zero
    while (((Get-Date) - $t0).TotalSeconds -lt 240) {
        Start-Sleep -Milliseconds 25
        if ($fen -eq [IntPtr]::Zero) { $fen = [Barre]::Fenetre($p.Id); if ($fen -eq [IntPtr]::Zero) { continue } }
        $s = [Barre]::Texte($fen)
        if (-not $s) { continue }
        $t = ((Get-Date) - $t0).TotalSeconds
        if (-not $etats.Contains($s) -and $etats.Count -lt 14) { $etats[$s] = $t }
        if ($s -match '^\s*([\d\s\u00a0\u202f\.,]+)\s*(objets?|objects?)\b') {
            $n = [int64](($Matches[1] -replace '[^\d]', ''))
            if ($n -ge $Minimum) { $final = $t; $objets = $n; break }   # total complet (la barre affiche des comptes partiels avant)
        }
    }
    "essai {0} : {1} objets, total atteint apres {2:N2} s" -f $i, $objets, $final
    foreach ($k in $etats.Keys) { "     {0,6:N2} s : {1}" -f $etats[$k], $k }
    if ($final) { $temps += $final }
    Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
    Start-Sleep 2
}
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
if ($temps.Count) { "meilleur temps : {0:N2} s ; moyenne : {1:N2} s" -f ($temps | Measure-Object -Minimum).Minimum, ($temps | Measure-Object -Average).Average }
