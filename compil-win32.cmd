@echo off
rem Compile Fouine pour Windows 32 bits. Resultat : dist\fouine-win32.exe
rem Prerequis : Rust (https://rustup.rs) et les "Outils de generation Visual Studio" (charge de travail C++,
rem avec les outils x86 : composant "MSVC ... outils de build x64/x86").
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if errorlevel 1 (
    echo Rust est introuvable. Installez-le depuis https://rustup.rs puis relancez ce fichier.
    exit /b 1
)
rustup target add i686-pc-windows-msvc
cargo build --release --locked -p fouine-app --target i686-pc-windows-msvc
if errorlevel 1 (
    echo La compilation a echoue. Verifiez que les Outils de generation Visual Studio avec la charge de travail C++ et les outils x86 sont installes.
    exit /b 1
)
if not exist dist mkdir dist
copy /y "target\i686-pc-windows-msvc\release\fouine.exe" "dist\fouine-win32.exe" >nul
echo.
echo Termine : dist\fouine-win32.exe
