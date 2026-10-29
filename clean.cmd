@echo off
rem Nettoie le depot : supprime les fichiers produits par la compilation (dossiers target et dist).
rem Les sources ne sont jamais touchees. Pour Windows ; voir clean.sh pour macOS et Linux.
setlocal
cd /d "%~dp0"
where cargo >nul 2>nul
if not errorlevel 1 (
    cargo clean
)
if exist target rmdir /s /q target
if exist dist rmdir /s /q dist
echo Nettoyage termine.
