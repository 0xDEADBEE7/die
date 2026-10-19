#!/bin/sh
# Does replacing the store's executable while it runs hurt the store?
# Compares an in-place `cp` (same inode) with copy-then-rename (new inode).
# usage: scripts/replace-binary.sh SCRATCH_DIR
set -u
SPIKE=$(cd "$(dirname "$0")/.." && pwd)
BASE=$1
N="$SPIKE/target/release/die"
N1="$SPIKE/target/n1/release/die"
for mode in cp-inplace cp-rename; do
  D="$BASE/replace-$mode"
  rm -rf "$D"; mkdir -p "$D/bin"; chmod 700 "$D"
  cp "$N" "$D/bin/die"
  cd "$D" || exit 1
  "$D/bin/die" --fd-store --dir "$D" --daemon --idle-exit 3 --sweep 1
  sleep 0.2
  SP=$(cat "$D/store.lock")
  "$N" host spawn-only --dir "$D" --run "rp$mode" --n 1 --prefix rp
  SH=$(sed -E 's/.*"pid":([0-9]+).*/\1/' "$D/spawned-rp.json")
  INODE0=$(stat -f %i "$D/bin/die")
  if [ "$mode" = cp-inplace ]; then
    cp "$N1" "$D/bin/die"
  else
    cp "$N1" "$D/bin/die.new" && mv "$D/bin/die.new" "$D/bin/die"
  fi
  INODE1=$(stat -f %i "$D/bin/die")
  OK=0
  for i in 1 2 3 4 5 6; do
    "$N" host list --dir "$D" --run x --tag "$i" 2>/dev/null && OK=$((OK+1))
    sleep 0.5
  done
  STORE=$(kill -0 "$SP" 2>/dev/null && echo alive || echo dead)
  SHELL=$(kill -0 "$SH" 2>/dev/null && echo alive || echo dead)
  echo "$mode: inode $INODE0 -> $INODE1; store $SP $STORE after 3 s of requests ($OK/6 LIST ok); shell $SH $SHELL; hup lines: $(cat "$D"/hup-rp_0.txt 2>/dev/null | wc -l | tr -d ' ')"
  tail -3 "$D/store.log" | sed 's/^/    log: /'
  "$N" host kill-all --dir "$D" --run x 2>/dev/null
  kill "$SP" 2>/dev/null
  kill -9 "$SH" 2>/dev/null
done
exit 0
