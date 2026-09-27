#!/bin/sh
# Meson custom_target helper: cargo build + copy binary to @OUTPUT@.
# Args: <cargo> <binary_name> <rust_target> <cargo_target_dir> <output> -- [cargo args...]
set -eu

cargo_bin="$1"
binary_name="$2"
rust_target="$3"
cargo_target="$4"
output="$5"
shift 5

# Optional "--" separator before cargo args
if [ "${1:-}" = "--" ]; then
  shift
fi

"$cargo_bin" build "$@"

src="$cargo_target/$rust_target/$binary_name"
cp -f "$src" "$output"
