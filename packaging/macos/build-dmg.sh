#!/usr/bin/env bash
# Build a compressed macOS drag-to-Applications .dmg from a packaged .app bundle.
# Must be run on macOS (requires `hdiutil` and `ditto`).
#
# Usage: packaging/macos/build-dmg.sh <app_bundle> <version> <out_dir> [arch]
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
  echo "Usage: $0 <app_bundle> <version> <out_dir> [arch]" >&2
  exit 2
fi

app_bundle=$1
version=$2
out_dir=$3
arch=${4:-arm64}

[[ -d "$app_bundle" ]] || { echo "error: app bundle not found: $app_bundle" >&2; exit 1; }
for cmd in hdiutil ditto; do
  command -v "$cmd" >/dev/null || { echo "error: required macOS tool '$cmd' not found" >&2; exit 1; }
done

app_name=$(basename "$app_bundle")
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

dmg_root="$stage/dmg"
mkdir -p "$dmg_root"
ditto "$app_bundle" "$dmg_root/$app_name"
ln -s /Applications "$dmg_root/Applications"

dmg_path="$out_dir/xcpc-workstation-${version}-macos-${arch}.dmg"
rm -f "$dmg_path"

hdiutil create \
  -volname "XCPC Workstation ${version}" \
  -srcfolder "$dmg_root" \
  -ov \
  -format UDZO \
  "$dmg_path" >/dev/null

echo "Built macOS DMG: $dmg_path"
