# Lance le test NTFS reel en administrateur (une fenetre UAC s'ouvre : accepter). Resultat dans target\ntfs-test.txt.
$e = Split-Path $PSScriptRoot
$cmd = "call `"$(Split-Path (Split-Path $e))\env.cmd`" && cd /d `"$e`" && cargo test -p fouine-core --test matrix ntfs_volume_reel_administrateur -- --ignored --nocapture > target\ntfs-test.txt 2>&1"
Start-Process cmd -ArgumentList '/c', $cmd -Verb RunAs -Wait
Get-Content "$e\target\ntfs-test.txt" -Tail 15
