#!/usr/bin/env bash
# Stage `osd` as a Tauri sidecar, so the installer carries the terminal client.
#
#   scripts/dev/build-osd-sidecar.sh <rust-target>
#
# Unlike the other three sidecars this one is not fetched — it is OURS, and it
# compiles the web client into itself (crates/osd-cli/build.rs), so the frontend
# has to exist BEFORE it is built. `tauri build` builds the frontend too, but it
# does so after this point, hence the explicit build here: an `osd` staged
# against a stale `dist/` would serve a stale UI to every browser that reaches
# it. Cargo and Vite both no-op when nothing changed, so the double build costs
# almost nothing.
#
# Tauri strips the target triple when it bundles. The product prefix keeps the
# Linux package from claiming the user's global `osd` command in `/usr/bin`.
set -euo pipefail

target="${1:?usage: build-osd-sidecar.sh <rust-target>}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

ext=""
case "$target" in *windows*) ext=".exe" ;; esac

echo "Building the frontend so osd embeds the current web client..."
(cd "$root" && pnpm --filter @ai4s/desktop build)

echo "Building osd for ${target}..."
cargo build --release --target "$target" --package osd-cli --manifest-path "$root/Cargo.toml"

dest_dir="$root/apps/desktop/src-tauri/binaries"
mkdir -p "$dest_dir"
cp "$root/target/$target/release/osd$ext" "$dest_dir/happy-science-osd-$target$ext"
chmod +x "$dest_dir/happy-science-osd-$target$ext"
ls -la "$dest_dir/happy-science-osd-$target$ext"
