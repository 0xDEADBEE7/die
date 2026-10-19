#!/bin/sh
# Spike S2 A4/A5: cold build of DIE's exact wiring in a clean environment, with a fake
# `zig` first on PATH that records any call; then the battery, the tests and `otool -L`.
# usage: clean-build.sh <scratch-dir> <cargo-home>
#   <cargo-home> must already hold the fetched dependencies (`cargo fetch` ran before).
set -eu
SPIKE=$(cd "$(dirname "$0")/.." && pwd)
W=$1
CH=$2
rm -rf "$W/fakebin" "$W/cold-target" "$W/ZIG_WAS_CALLED"
mkdir -p "$W/fakebin"
cat > "$W/fakebin/zig" <<EOF
#!/bin/sh
echo "zig invoked: \$*" >> "$W/ZIG_WAS_CALLED"
exit 99
EOF
chmod +x "$W/fakebin/zig"
cd "$SPIKE"
echo "== env -i cold build (fake zig first on PATH)"
env -i HOME="$HOME" CARGO_HOME="$CH" CARGO_TARGET_DIR="$W/cold-target" \
    PATH="$W/fakebin:$HOME/.cargo/bin:/usr/bin:/bin" \
    cargo build --release --offline 2>&1 | tail -3
if [ -e "$W/ZIG_WAS_CALLED" ]; then echo "ZIG WAS CALLED:"; cat "$W/ZIG_WAS_CALLED"; else echo "zig was never called"; fi
echo "== which C compilers ran: build scripts in the graph"
ls "$W/cold-target/release/build" | sed 's/-[0-9a-f]*$//' | sort -u
echo "== battery (cold-build binary)"
"$W/cold-target/release/battery" > "$W/battery.out" 2>&1 || true
grep -E '^TOTAL|^S2 WIRING|FAIL' "$W/battery.out"
echo "== tests (same environment)"
env -i HOME="$HOME" CARGO_HOME="$CH" CARGO_TARGET_DIR="$W/cold-target" \
    PATH="$W/fakebin:$HOME/.cargo/bin:/usr/bin:/bin" \
    cargo test --release --offline 2>&1 | grep -E '^test |test result: .* [1-9][0-9]* passed|FAILED|panicked' || true
if [ -e "$W/ZIG_WAS_CALLED" ]; then echo "ZIG WAS CALLED (tests)"; fi
echo "== otool -L"
for b in battery s2; do echo "$b:"; otool -L "$W/cold-target/release/$b" | tail -n +2; done
echo "== the wiring alone (no libc crate): tools/wiring-only builds the same battery source"
cp "$SPIKE/Cargo.lock" "$SPIKE/tools/wiring-only/Cargo.lock"
(cd "$SPIKE/tools/wiring-only" && env -i HOME="$HOME" CARGO_HOME="$CH" CARGO_TARGET_DIR="$W/wiring-target" \
    PATH="$W/fakebin:$HOME/.cargo/bin:/usr/bin:/bin" cargo build --release --offline 2>&1 | tail -1)
if [ -e "$W/ZIG_WAS_CALLED" ]; then echo "ZIG WAS CALLED (wiring-only)"; else echo "zig was never called"; fi
otool -L "$W/wiring-target/release/battery" | tail -n +2
"$W/wiring-target/release/battery" > "$W/battery-wiring-only.out" 2>&1 || true
grep -E '^TOTAL|^S2 WIRING' "$W/battery-wiring-only.out"
