#!/usr/bin/env bash
# Regenerate demo.gif reproducibly.
#
# The popup and browser are separate plugin instances that each request
# permission, and Zellij only persists a grant on graceful exit. To avoid an
# approval prompt mid-recording, we pre-seed a writable permission cache
# (redirected via XDG_CACHE_HOME) keyed by the wasm's absolute path, then drive
# the UI with vhs. Absolute paths are used because that key format is the one
# Zellij persists (see ~/.cache/zellij/permissions.kdl on a real install).
set -euo pipefail
cd "$(dirname "$0")"

wasm="$PWD/target/wasm32-wasip1/release/zj_which_key.wasm"

echo "Building wasm..."
cargo build --release --target wasm32-wasip1

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Config with the plugin referenced by absolute path (both the controller and
# the browser keybind), so the permission key is stable and pre-grantable.
sed "s#file:\./target/wasm32-wasip1/release/zj_which_key.wasm#file:$wasm#g" \
    examples/config.kdl > "$work/config.kdl"

# Pre-approve: key is the absolute path WITHOUT the file: scheme.
mkdir -p "$work/cache/zellij"
cat > "$work/cache/zellij/permissions.kdl" <<EOF
"$wasm" {
    ReadApplicationState
    ChangeApplicationState
    MessageAndLaunchOtherPlugins
}
EOF

# Point the tape at the generated config.
sed "s#{{CONFIG}}#$work/config.kdl#g" demo.tape > "$work/demo.tape"

echo "Recording demo.gif..."
XDG_CACHE_HOME="$work/cache" vhs "$work/demo.tape"

echo "Done: demo.gif"
