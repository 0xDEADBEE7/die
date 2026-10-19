#!/bin/sh
# Fetches Ioskeley Mono Term v2.1.0 (Regular, Bold, Italic, BoldItalic) for the
# S1 spike without installing anything. The zip goes into a fresh temporary
# directory, its SHA-256 is checked, and only the four .ttf files are extracted
# to OUT (default: ./fonts next to this script). Nothing in the zip is executed.
set -eu
OUT="${1:-$(cd "$(dirname "$0")" && pwd)/fonts}"
URL="https://github.com/ahatem/IoskeleyMono/releases/download/v2.1.0/IoskeleyMono-Term.zip"
SHA="a45f766d60fb7f1ed244ecd456a890d68cad8bd480f7803325d04133995bc000"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL -A "Mozilla/5.0 (generic)" -o "$TMP/term.zip" "$URL"
GOT="$(shasum -a 256 "$TMP/term.zip" | cut -d' ' -f1)"
[ "$GOT" = "$SHA" ] || { echo "checksum mismatch: $GOT" >&2; exit 1; }
mkdir -p "$OUT"
for f in Regular Bold Italic BoldItalic; do
  unzip -o -j -q "$TMP/term.zip" "Normal/Hinted/IoskeleyMonoTerm-$f.ttf" -d "$OUT"
done
ls -l "$OUT"
