#!/usr/bin/env bash
set -euo pipefail

version="${EMULATORJS_VERSION:-v4.2.3}"
repo="EmulatorJS/EmulatorJS"
api_url="https://api.github.com/repos/${repo}/releases/tags/${version}"
workdir="$(mktemp -d)"
dest="frontend/emulatorjs"

cleanup() {
  rm -rf "$workdir"
}
trap cleanup EXIT

mkdir -p "$dest"

echo "Fetching EmulatorJS ${version} release metadata"
curl -fsSL "$api_url" -o "$workdir/release.json"

asset_url="$(
  python3 - "$workdir/release.json" <<'PY'
import json
import sys

release = json.load(open(sys.argv[1], encoding="utf-8"))
assets = release.get("assets", [])
for asset in assets:
    name = asset.get("name", "")
    url = asset.get("browser_download_url", "")
    if (name.endswith(".zip") or name.endswith(".7z")) and url:
        print(url)
        break
else:
    raise SystemExit("no supported release asset found")
PY
)"

echo "Downloading ${asset_url}"
asset_name="${asset_url##*/}"
curl -fL "$asset_url" -o "$workdir/$asset_name"

echo "Unpacking release"
mkdir -p "$workdir/unpacked"
case "$asset_name" in
  *.zip)
    unzip -q "$workdir/$asset_name" -d "$workdir/unpacked"
    ;;
  *.7z)
    7z x "$workdir/$asset_name" "-o$workdir/unpacked" >/dev/null
    ;;
  *)
    echo "unsupported EmulatorJS asset: $asset_name" >&2
    exit 1
    ;;
esac

data_dir="$(
  find "$workdir/unpacked" -type f -name loader.js -path '*/data/loader.js' -print -quit |
    sed 's#/loader.js$##'
)"

if [ -z "$data_dir" ]; then
  echo "could not find data/loader.js in EmulatorJS release asset" >&2
  exit 1
fi

rm -rf "$dest/data"
mkdir -p "$dest"
cp -R "$data_dir" "$dest/data"
printf '%s\n' "$version" > "$dest/VERSION"

echo "Vendored EmulatorJS ${version} into ${dest}/data"
