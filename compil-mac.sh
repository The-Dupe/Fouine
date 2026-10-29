#!/usr/bin/env sh
# Compile Fouine pour macOS (binaire universel Apple Silicon + Intel). Resultat : dist/fouine-mac
# Prerequis : Rust (https://rustup.rs) et les outils en ligne de commande Xcode (xcode-select --install).
set -eu
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust est introuvable. Installez-le depuis https://rustup.rs puis relancez ce script." >&2
    exit 1
fi

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked -p fouine-app --target aarch64-apple-darwin
cargo build --release --locked -p fouine-app --target x86_64-apple-darwin

mkdir -p dist
lipo -create -output dist/fouine-mac \
    target/aarch64-apple-darwin/release/fouine \
    target/x86_64-apple-darwin/release/fouine
chmod +x dist/fouine-mac

echo
echo "Termine : dist/fouine-mac"
echo "Au premier lancement, macOS peut demander une confirmation (clic droit > Ouvrir)."
