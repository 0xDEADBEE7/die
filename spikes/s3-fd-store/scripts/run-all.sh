#!/bin/sh
# Build the three protocol variants and run every S3 check and probe.
# usage: scripts/run-all.sh SCRATCH_DIR     (SCRATCH_DIR must be private: 0700)
set -eu
SPIKE=$(cd "$(dirname "$0")/.." && pwd)
BASE=$1
mkdir -p "$BASE" && chmod 700 "$BASE"
cd "$SPIKE"
cargo build --release --offline
cargo build --release --offline --features proto-n1 --target-dir target/n1
cargo build --release --offline --features proto-major2 --target-dir target/m2
DIE="$SPIKE/target/release/die"
mkdir -p "$BASE/probe" "$BASE/race" && chmod 700 "$BASE/race"
"$DIE" probe "$BASE/probe" > "$BASE/probe/probe.txt"
"$DIE" ctty "$BASE/race" > "$BASE/race/ctty.txt"
"$DIE" race "$BASE/race" > "$BASE/race/race.txt"
"$SPIKE/scripts/replace-binary.sh" "$BASE" > "$BASE/replace-binary.txt" 2>&1
cd "$BASE"
"$DIE" check all --base "$BASE" --n1 "$SPIKE/target/n1/release/die" --m2 "$SPIKE/target/m2/release/die" --app-dir "$SPIKE/target/app" > "$BASE/summary-all.json"
cat "$BASE/probe/probe.txt" "$BASE/race/ctty.txt" "$BASE/race/race.txt" "$BASE/replace-binary.txt"
echo "summaries: $BASE/summary-*.json"
