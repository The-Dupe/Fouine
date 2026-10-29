@echo off
rem Compile Fouine pour Windows 64 bits. Resultat : dist\fouine-win64.exe
rem Prerequis : Rust (https://rustup.rs) et les "Outils de generation Visual Studio" (charge de travail C++).
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust est introuvable. Installez-le depuis https://rustup.rs puis relancez ce fichier.
    exit /b 1
)
rustup target add x86_64-pc-windows-msvc
cargo build --release --locked -p fouine-app --target x86_64-pc-windows-msvc
if errorlevel 1 (
    echo La compilation a echoue. Verifiez que les Outils de generation Visual Studio avec la charge de travail C++ sont installes.
    exit /b 1
)
if not exist dist mkdir dist
copy /y "target\x86_64-pc-windows-msvc\release\fouine.exe" "dist\fouine-win64.exe" >nul
echo.
echo Termine : dist\fouine-win64.exe
