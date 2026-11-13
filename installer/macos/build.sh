#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
version=""
binary="$root/target/aarch64-apple-darwin/release/grana"
output="$root/dist"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            version="$2"
            shift 2
            ;;
        --binary)
            binary="$2"
            shift 2
            ;;
        --output)
            output="$2"
            shift 2
            ;;
        *)
            echo "usage: build.sh [--version <x.y.z>] [--binary <path>] [--output <dir>]" >&2
            exit 2
            ;;
    esac
done

[[ "$(uname -s)" == "Darwin" ]] || {
    echo "the macOS installer requires macOS" >&2
    exit 1
}

if [[ -z "$version" ]]; then
    version="$(cd "$root" && cargo metadata --no-deps --format-version 1 --locked |
        python3 -c 'import json, sys; print(next(package["version"] for package in json.load(sys.stdin)["packages"] if package["name"] == "grana"))')"
fi
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
    echo "the installer version must be numeric: $version" >&2
    exit 1
}
[[ -f "$binary" ]] || {
    echo "the release binary was not found: $binary" >&2
    exit 1
}
[[ -f "$root/installer/macos/art/Grana.icns" ]] || {
    echo "the installer artwork was not found" >&2
    exit 1
}

stage="$root/target/installer/macos"
app="$stage/Grana.app"
dmg="$stage/Grana.dmg"
rm -rf "$stage"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/grana"
chmod 755 "$app/Contents/MacOS/grana"
architectures="$(lipo -archs "$app/Contents/MacOS/grana")"
[[ "$architectures" == "arm64" ]] || {
    echo "the macOS installer requires an arm64 binary: $architectures" >&2
    exit 1
}
cp "$root/installer/macos/Info.plist" "$app/Contents/Info.plist"
cp "$root/installer/macos/art/Grana.icns" "$app/Contents/Resources/Grana.icns"
plutil -replace CFBundleShortVersionString -string "$version" "$app/Contents/Info.plist"
plutil -replace CFBundleVersion -string "$version" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

identity="${GRANA_SIGN_IDENTITY:-}"
if [[ -n "$identity" ]]; then
    codesign --force --options runtime --timestamp --sign "$identity" "$app"
else
    codesign --force --sign - "$app"
fi
codesign --verify --strict "$app"


venv="$stage/venv"
python3 -m venv "$venv"
"$venv/bin/python" -m pip install --quiet --disable-pip-version-check "dmgbuild==1.6.7"
"$venv/bin/dmgbuild" -s "$root/installer/macos/dmg.py" \
    -D app="$app" \
    -D background="$root/installer/macos/art/dmg-background.png" \
    "Grana" "$dmg"

if [[ -n "$identity" ]]; then
    codesign --force --timestamp --sign "$identity" "$dmg"
fi

notary_key="${GRANA_NOTARY_KEY:-}"
if [[ -n "$identity" && -s "$notary_key" ]]; then
    xcrun notarytool submit "$dmg" \
        --key "$notary_key" \
        --key-id "${GRANA_NOTARY_KEY_ID:?}" \
        --issuer "${GRANA_NOTARY_ISSUER:?}" \
        --wait
    xcrun stapler staple "$dmg"
    xcrun stapler validate "$dmg"
fi

mkdir -p "$output"
name="Grana-$version-macos-arm64.dmg"
cp "$dmg" "$output/$name"
echo "installer: $output/$name"


