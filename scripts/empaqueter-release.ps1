# Prepare les fichiers de release dans dist\ : un zip par architecture Windows, ne contenant QUE fouine.exe
# (dist ne contient rien d'autre ; les sommes SHA-256 sont seulement affichees). Ne publie rien.
#   -Win32 : produit aussi la version 32 bits (exige les bibliotheques MSVC x86).
# Dans le dossier portable (env.cmd trouve en remontant), utilise la chaine portable ; sinon, la chaine du systeme.
param([switch]$Win32)
$ErrorActionPreference = 'Stop'
$e = Split-Path $PSScriptRoot
$version = (Select-String -Path "$e\Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$racine = $null; $d = $e
while ($d) { if (Test-Path "$d\env.cmd") { $racine = $d; break }; $d = Split-Path $d }
$dist = "$e\dist"
New-Item -ItemType Directory -Force $dist | Out-Null

function Compiler([string]$cible, [string]$prefixe) {
    $env = if ($racine) { "call `"$racine\env.cmd`" && " } else { '' }
    # Aucun chemin de la machine de compilation dans l'executable (messages d'erreur, panics) : on remplace les
    # racines par des noms neutres. RUSTFLAGS remplace les options du fichier .cargo\config.toml : on y remet
    # l'edition statique du runtime C. Le plus specifique en dernier (le dernier motif qui correspond l'emporte).
    $flags = '-C target-feature=+crt-static'
    if ($racine) {
        $flags += " --remap-path-prefix=$racine=portable"
        $flags += " --remap-path-prefix=$racine\tools\rust\cargo=cargo --remap-path-prefix=$racine\tools\rust\rustup=rustup"
    }
    $flags += " --remap-path-prefix=$e=fouine"
    cmd /c "$env$prefixe set `"RUSTFLAGS=$flags`" && cd /d `"$e`" && cargo build --release --locked -p fouine-app --target $cible" | Select-Object -Last 3
    if ($LASTEXITCODE -ne 0) { throw "compilation $cible echouee" }
}

function Empaqueter([string]$cible, [string]$nom) {
    $exe = "$e\target\$cible\release\fouine.exe"
    $zip = "$dist\Fouine-$version-$nom.zip"
    $tmp = "$e\target\empaquetage-$nom"
    Remove-Item $tmp, $zip -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory $tmp | Out-Null
    Copy-Item $exe "$tmp\fouine.exe"
    Compress-Archive -Path "$tmp\fouine.exe" -DestinationPath $zip -CompressionLevel Optimal
    Remove-Item $tmp -Recurse -Force
    $h = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    "{0} : {1:N1} Mo (exe {2:N1} Mo) sha256 {3}" -f (Split-Path $zip -Leaf), ((Get-Item $zip).Length / 1MB), ((Get-Item $exe).Length / 1MB), $h
}

Compiler 'x86_64-pc-windows-msvc' ''
Empaqueter 'x86_64-pc-windows-msvc' 'portable-win64'

if ($Win32) {
    # Chaine portable : editeur de liens dedie a la cible 32 bits (bibliotheques x86) ; sinon, celle du systeme.
    $lien = if ($racine) { "$racine\tools\rust\msvc-x86\link-x86.cmd" } else { $null }
    $prefixe = if ($lien -and (Test-Path $lien)) { "set `"CARGO_TARGET_I686_PC_WINDOWS_MSVC_LINKER=$lien`" &&" } else { '' }
    Compiler 'i686-pc-windows-msvc' $prefixe
    Empaqueter 'i686-pc-windows-msvc' 'portable-win32'
}
"Contenu de dist :"; Get-ChildItem $dist | Select-Object Name, Length | Format-Table -AutoSize | Out-String
