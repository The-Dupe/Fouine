#!/usr/bin/env sh
# Nettoie le depot : supprime les fichiers produits par la compilation (dossiers target et dist).
# Les sources ne sont jamais touchees. Pour macOS et Linux ; voir clean.cmd pour Windows.
set -eu
cd "$(dirname "$0")"

if command -v cargo >/dev/null 2>&1; then
    cargo clean
fi
rm -rf target dist

echo "Nettoyage termine."
