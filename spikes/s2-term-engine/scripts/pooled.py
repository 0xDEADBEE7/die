"""Pooled percentiles over the raw samples written by run-reps.sh.
usage: /usr/bin/python3 -I pooled.py <results.jsonl>"""
import json
import sys

rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip().startswith("{")]


def pct(v, p):
    v = sorted(v)
    return v[round((len(v) - 1) * p)] / 1000.0 if v else float("nan")


def show(name, key, wl):
    print(f"== {name}")
    res = {}
    for sh in ("inproc", "helper"):
        rs = [r for r in rows if r["workload"] == wl and r["shape"] == sh and key in r]
        allv = [x for r in rs for x in r[key]]
        per = [pct(r[key], 0.99) for r in rs]
        res[sh] = (pct(allv, 0.5), pct(allv, 0.99), max(allv) / 1000.0 if allv else 0)
        print(f"  {sh:7s} reps={len(rs)} n={len(allv)} pooled p50 {res[sh][0]:.1f} us  p99 {res[sh][1]:.1f} us  max {res[sh][2]:.1f} us"
              f" | per-rep p99: {', '.join(f'{x:.0f}' for x in per)}")
    if "inproc" in res and "helper" in res:
        print(f"  added by helper: p50 {res['helper'][0]-res['inproc'][0]:+.1f} us, p99 {res['helper'][1]-res['inproc'][1]:+.1f} us")


show("w1 key -> frame available (end to end, real bash echo)", "raw_key_to_frame_ns", "w1")
show("w1 PTY read -> frame available on DIE's main thread", "raw_pty_read_to_available_ns", "w1")
show("w3 PTY write -> frame available (250x70 full redraw, 60 fps)", "raw_write_to_available_ns", "w3")
show("w3 main-thread ingest per vsync", "raw_vsync_ingest_ns", "w3")
show("w2 main-thread ingest per vsync (200 MB flood)", "raw_vsync_ingest_ns", "w2")
for sh in ("inproc", "helper"):
    rs = [r for r in rows if r["workload"] == "w2" and r["shape"] == sh]
    over = sum(r["ingest"]["vsyncs_over_1ms"] for r in rs)
    n = sum(r["ingest"]["per_vsync"]["n"] for r in rs)
    mbs = [r["mb_per_s"] for r in rs]
    print(f"  w2 {sh}: vsyncs over 1 ms: {over} of {n}; MiB/s per rep: {', '.join(f'{x:.0f}' for x in mbs)}")
for sh in ("inproc", "helper"):
    rs = [r for r in rows if r["workload"] == "w3" and r["shape"] == sh]
    print(f"  w3 {sh}: frames seen per rep: {', '.join(str(r['frames_seen']) for r in rs)}; coalesced: {', '.join(str(r['coalesced']) for r in rs)}")
