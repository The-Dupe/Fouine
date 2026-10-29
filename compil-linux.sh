#!/usr/bin/env sh
# Compile Fouine pour Linux (architecture de la machine). Resultat : dist/fouine-linux
# Prerequis : Rust (https://rustup.rs), un compilateur C (gcc ou clang) et pkg-config.
# A l'execution : un pilote graphique Vulkan ou OpenGL, et X11 ou Wayland (libxkbcommon).
set -eu
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Rust est introuvable. Installez-le depuis https://rustup.rs puis relancez ce script." >&2
    exit 1
fi

cargo build --release --locked -p fouine-app

mkdir -p dist
cp target/release/fouine dist/fouine-linux
chmod +x dist/fouine-linux

echo
echo "Termine : dist/fouine-linux"
