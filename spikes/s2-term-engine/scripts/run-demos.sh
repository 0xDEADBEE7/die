#!/bin/sh
# Spike S2 part B helper demos: (i) kill -9 of a helper (plus the PTY-holder layout),
# (ii) kill -9 of DIE and reattach (same build, then build N -> build N+1), (iii) versions.
# usage: run-demos.sh <spike-dir> <scratch-dir> <cargo-home>
set -u
SPIKE=$1
W=$2
CH=$3
mkdir -p "$W"
cd "$SPIKE"
echo "== (i) kill -9 of one helper"
./target/release/s2 demo-kill --run-dir "$W/kill"
echo "== (ii) kill -9 of DIE, reattach (same build)"
sh scripts/demo-reattach.sh "$SPIKE/target/release/s2" "$W/reattach"
echo "== (ii') helper from build N, new DIE from build N+1"
mkdir -p "$W/builds"
S2_BUILD_TAG=buildN CARGO_HOME="$CH" cargo build --release --offline --bin s2 2>&1 | tail -1
cp target/release/s2 "$W/builds/s2-N"
S2_BUILD_TAG=buildN1 CARGO_HOME="$CH" cargo build --release --offline --bin s2 2>&1 | tail -1
cp target/release/s2 "$W/builds/s2-N1"
sh scripts/demo-reattach.sh "$W/builds/s2-N" "$W/reattach-xbuild" "$W/builds/s2-N1"
CARGO_HOME="$CH" cargo build --release --offline --bin s2 2>&1 | tail -1
echo "== (iii) version handshake"
./target/release/s2 demo-version --run-dir "$W/version"
