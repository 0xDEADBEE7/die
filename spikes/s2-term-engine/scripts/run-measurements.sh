#!/bin/sh
# Spike S2 part B: every workload on both shapes, interleaved, JSON lines into <data-dir>.
# usage: run-measurements.sh <s2-binary> <s2-binary-with-frameworks> <data-dir>
set -u
S2=$1
S2FW=$2
OUT=$3
mkdir -p "$OUT/run"
R="$OUT/run"
J="$OUT/results.jsonl"
note() { echo "# $(date '+%H:%M:%S') $* | load $(uptime | sed 's/.*load averages*: //')" | tee -a "$OUT/log.txt"; }
b() { "$S2" bench --run-dir "$R" --out "$J" "$@" > /dev/null 2>> "$OUT/stderr.txt" || echo "FAILED: $*" | tee -a "$OUT/log.txt"; }
note start
for rep in 1 2; do
  for sh in inproc helper; do note "w1 $sh rep $rep"; b --shape $sh --workload w1 --n 500 --rep $rep; done
done
for rep in 1 2; do
  for sh in inproc helper; do note "w3 $sh rep $rep"; b --shape $sh --workload w3 --frames 1000 --fps 60 --rep $rep; done
done
for sh in inproc helper; do note "w3 stress $sh (fps 1000)"; b --shape $sh --workload w3 --frames 1000 --fps 1000 --rep stress; done
for rep in 1 2; do
  for sh in inproc helper; do note "w2 $sh rep $rep"; b --shape $sh --workload w2 --bytes 209715200 --rep $rep; done
done
for rep in 1 2 3 4 5; do
  for k in rgba4k png1080; do
    for sh in inproc helper; do note "w4 $k $sh rep $rep"; b --shape $sh --workload w4 --kind $k --rep $rep; done
  done
done
for sh in inproc helper; do note "scrollback $sh"; b --shape $sh --workload scrollback; done
for sh in inproc helper; do note "rss $sh"; b --shape $sh --workload rss --n 8; done
note "rss helper, binary linking AppKit/Metal/QuartzCore/CoreText"
"$S2FW" bench --run-dir "$R" --out "$J" --shape helper --workload rss --n 8 --variant link-frameworks > /dev/null 2>> "$OUT/stderr.txt"
for sh in inproc helper; do note "w5 $sh (60 s idle)"; b --shape $sh --workload w5 --secs 60; done
note done
