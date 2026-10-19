#!/bin/sh
# Spike S2 step A1: derive DIE's vendored archive from the signed xcframework zip,
# with only Xcode CLT tools (ditto, lipo, strip, ld, libtool, nm). No Zig.
# usage: derive-archive.sh <ghostty-vt.xcframework.zip> <work-dir>
# Prints the path of the derived libghostty-vt.a on the last line.
set -eu
ZIP=$1
W=$2
export ZERO_AR_DATE=1
rm -rf "$W"
mkdir -p "$W/x" "$W/m"
ditto -x -k "$ZIP" "$W/x"
FAT="$W/x/ghostty-vt.xcframework/macos-arm64_x86_64/libghostty-vt.a"
lipo -thin arm64 "$FAT" -output "$W/thin.a"
strip -S "$W/thin.a" -o "$W/thin-stripped.a" 2>/dev/null || { cp "$W/thin.a" "$W/thin-stripped.a"; strip -S "$W/thin-stripped.a"; }
printf '_ghostty_*\n' > "$W/exports.txt"
ld -r -arch arm64 -platform_version macos 13.0 13.0 -all_load "$W/thin-stripped.a" \
   -exported_symbols_list "$W/exports.txt" -o "$W/ghostty-vt-prelinked.o"
libtool -static -o "$W/libghostty-vt.a" "$W/ghostty-vt-prelinked.o" 2>/dev/null
# Facts about the result
nm -gU "$W/libghostty-vt.a" 2>/dev/null | awk 'NF==3{print $3}' | sort -u > "$W/globals.txt"
nm -u "$W/libghostty-vt.a" 2>/dev/null | awk 'NF>=1{print $NF}' | grep -v ':$' | sort -u > "$W/undefined.txt"
echo "globals=$(wc -l < "$W/globals.txt" | tr -d ' ') non_ghostty_globals=$(grep -vc '^_ghostty_' "$W/globals.txt" || true) undefined=$(wc -l < "$W/undefined.txt" | tr -d ' ')"
echo "$W/libghostty-vt.a"
