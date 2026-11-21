#!/bin/bash
# Builds Witch It+ for Linux and Windows on Linux. Windows links with rust-lld against the
# MSVC libraries and Windows SDK of native-build/D (no MinGW, no Wine): their file names are
# mixed case, so a folder of lower-case links stands in for them.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
work=${WORK:-/mnt/SATA-1/Temp/WitchIt-claude}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$work/launcher-target}
sdk=$work/native-build/D
libs=$CARGO_TARGET_DIR/winlibs
if [ ! -d "$libs" ]; then
    mkdir -p "$libs"
    for d in "$sdk/Windows Kits/10/Lib/10.0.26100.0/um/x64" "$sdk/Windows Kits/10/Lib/10.0.26100.0/ucrt/x64" "$sdk/BuildTools2022/VC/Tools/MSVC/14.44.35207/lib/x64"; do
        for f in "$d"/*; do n=$(basename "$f" | tr 'A-Z' 'a-z'); [ -e "$libs/$n" ] || ln -s "$f" "$libs/$n"; done
    done
fi
cd "$here"
cargo test --release
cargo build --release
CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=rust-lld CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS="-Lnative=$libs" \
    cargo build --release --target x86_64-pc-windows-msvc
ls -la "$CARGO_TARGET_DIR/release/witchit-plus" "$CARGO_TARGET_DIR/x86_64-pc-windows-msvc/release/witchit-plus.exe"
