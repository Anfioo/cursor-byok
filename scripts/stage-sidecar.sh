#!/usr/bin/env bash
# Builds cursor-server and stages it where the Tauri shell expects its sidecar:
#
#   apps/desktop/src-tauri/binaries/cursor-server-<host-triple>[.exe]
#
# tauri-build fails the desktop build while that file is missing, and the
# directory is git-ignored, so every machine or CI job that compiles
# apps/desktop/src-tauri has to stage the sidecar first.
#
#   bash scripts/stage-sidecar.sh            # debug, for `tauri dev` and CI checks
#   bash scripts/stage-sidecar.sh release    # release, for installers
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

profile=${1:-debug}
case "$profile" in
  debug)
    cargo_flags=()
    target_dir=debug
    ;;
  release)
    cargo_flags=(--release)
    target_dir=release
    ;;
  *)
    echo "usage: $0 [debug|release]" >&2
    exit 2
    ;;
esac

triple=$(rustc -vV | sed -n 's/^host: //p')
case "$triple" in
  *windows*) exe=.exe ;;
  *) exe= ;;
esac

sidecar_dir=apps/desktop/src-tauri/binaries
sidecar=$sidecar_dir/cursor-server-$triple$exe

echo "==> building cursor-server ($profile) for $triple"
cargo build --package cursor-server --bin cursor-server ${cargo_flags[@]+"${cargo_flags[@]}"}

mkdir -p "$sidecar_dir"
cp -f "target/$target_dir/cursor-server$exe" "$sidecar"
echo "==> sidecar ready: $sidecar"
