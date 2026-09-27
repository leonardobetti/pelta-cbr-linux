#!/usr/bin/env bash
# Fail if the Flatpak manifest's finish-args mention x11 (x11 or fallback-x11),
# or lack --socket=wayland. Pelta on Linux is Wayland only.
# Usage: bash build-aux/check-flatpak-no-x11.sh [manifest]
set -euo pipefail

manifest="${1:-packaging/com.pelta.ComicReader.yml}"

if [[ ! -f "$manifest" ]]; then
  echo "check-flatpak-no-x11: manifest not found: $manifest" >&2
  exit 1
fi

# finish-args block: from "finish-args:" to the next top-level key.
args=$(awk '/^finish-args:/{f=1;next} f&&/^[^[:space:]-]/{f=0} f' "$manifest")
echo "$args"

if echo "$args" | grep -qi 'x11'; then
  echo "::error file=$manifest::finish-args must not mention x11 (Wayland only)" >&2
  exit 1
fi
if ! echo "$args" | grep -q -- '--socket=wayland'; then
  echo "::error file=$manifest::finish-args must include --socket=wayland" >&2
  exit 1
fi
echo "check-flatpak-no-x11: OK (Wayland only)"
