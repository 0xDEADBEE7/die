#!/bin/sh
# Spike S2: extra interleaved repetitions of the latency and flood workloads, with raw samples.
# usage: run-reps.sh <s2-binary> <data-dir> [reps]
set -u
S2=$1
OUT=$2
N=${3:-5}
mkdir -p "$OUT/run"
note() { echo "# $(date '+%H:%M:%S') $* | load $(uptime | sed 's/.*load averages*: //')" | tee -a "$OUT/log.txt"; }
b() { "$S2" bench --run-dir "$OUT/run" --out "$OUT/results.jsonl" "$@" > /dev/null 2>> "$OUT/stderr.txt" || note "FAILED: $*"; }
i=1
while [ $i -le "$N" ]; do
  for sh in inproc helper; do note "w1 $sh rep $i"; b --shape $sh --workload w1 --n 500 --rep $i; done
  for sh in helper inproc; do note "w3 $sh rep $i"; b --shape $sh --workload w3 --frames 1000 --fps 60 --rep $i; done
  for sh in inproc helper; do note "w2 $sh rep $i"; b --shape $sh --workload w2 --bytes 209715200 --rep $i; done
  i=$((i+1))
done
note done
