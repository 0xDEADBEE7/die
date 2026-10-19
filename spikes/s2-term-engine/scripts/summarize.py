"""Summarize results.jsonl from run-measurements.sh into the record's tables.
usage: /usr/bin/python3 -I summarize.py <results.jsonl>"""
import json
import sys

rows = [json.loads(l) for l in open(sys.argv[1]) if l.strip().startswith("{")]


def sel(wl, shape, **kw):
    return [r for r in rows if r["workload"] == wl and r["shape"] == shape and all(r.get(k) == v for k, v in kw.items())]


def fmt(s, keys=("p50_us", "p99_us", "max_us")):
    return " / ".join(f"{s[k]:.1f}" for k in keys) if s and s.get("n") else "-"


print("== w1 typing echo (us: p50 / p99 / max)")
for sh in ("inproc", "helper"):
    for r in sel("w1", sh):
        print(f"  {sh:7s} rep {r['rep']}: n={r['samples']} miss={r['misses']} key->frame {fmt(r['key_to_frame'])} | "
              f"pty read->frame available {fmt(r['pty_read_to_frame_available'])} | main ingest/vsync {fmt(r['ingest']['per_vsync'])}")

print("== w3 TUI 250x70 (us: p50 / p99 / max)")
for sh in ("inproc", "helper"):
    for r in sel("w3", sh):
        print(f"  {sh:7s} rep {r['rep']}: fps={r['fps']} seen={r['frames_seen']}/{r['frames_sent']} coalesced={r['coalesced']} "
              f"pty write->frame available {fmt(r['pty_write_to_frame_available'])} | pty read->available {fmt(r['pty_read_to_frame_available'])} | "
              f"main ingest/vsync {fmt(r['ingest']['per_vsync'])} over1ms={r['ingest']['vsyncs_over_1ms']} | socket MB={r['transport_bytes_in']/1e6:.1f}")

print("== w2 flood 200 MB")
for sh in ("inproc", "helper"):
    for r in sel("w2", sh):
        print(f"  {sh:7s} rep {r['rep']}: ok={r['ok']} {r['mb_per_s']:.1f} MiB/s to DIE in {r['to_die_s']:.2f}s; frames {r['frames']} "
              f"({r['frames_per_s']:.0f}/s, {r['avg_rows_per_frame']:.0f} rows) | main ingest/vsync {fmt(r['ingest']['per_vsync'])} "
              f"over1ms={r['ingest']['vsyncs_over_1ms']} of {r['ingest']['per_vsync']['n']} | socket MB={r['transport_bytes_in']/1e6:.1f}")

print("== w4 images (ms)")
for k in ("rgba4k", "png1080"):
    for sh in ("inproc", "helper"):
        rs = sel("w4", sh, kind=k)
        if not rs:
            continue
        h = sorted(r["handoff_ready_to_available_ms"] for r in rs)
        c = sorted(r["engine_copy_ms"] for r in rs)
        t = sorted(r["transport_ms"] for r in rs)
        m = sorted(r["main_thread_ingest_for_image_wake_us"] for r in rs)
        d = sorted(r["png_decode_ms"] for r in rs)
        g = sorted(r["gen_start_to_ready_ms"] for r in rs)
        ok = all(r["pixels_ok"] for r in rs)
        print(f"  {k:8s} {sh:7s} n={len(rs)} pixels_ok={ok} handoff median {h[len(h)//2]:.2f} max {h[-1]:.2f} | engine copy median {c[len(c)//2]:.2f} | "
              f"transport median {t[len(t)//2]:.3f} max {t[-1]:.3f} | main-thread wake median {m[len(m)//2]:.1f}us max {m[-1]:.1f}us | "
              f"png decode median {d[len(d)//2]:.2f} | gen start->ready median {g[len(g)//2]:.1f} | copies {rs[0]['engine_copies']} | "
              f"socket bytes during image median {sorted(r['transport_bytes_during_image'] for r in rs)[len(rs)//2]} | memory: {rs[0]['pixel_memory']}")

print("== w5 idle")
for sh in ("inproc", "helper"):
    for r in sel("w5", sh):
        print(f"  {sh:7s}: {r['secs']}s events={r['events_in_window']} die main fd wakeups={r['die_main_fd_wakeups']} "
              f"timer={r['die_main_timer_wakeups']} engine loop wakeups={r['engine_loop_wakeups']} die={r['die_process']} engine={r['engine_process']} shell={r['shell']}")

print("== RSS")
for r in [r for r in rows if r["workload"] == "rss"]:
    print(f"  {r['shape']:7s} {r['variant']:16s} n={r['n']} empty={r['empty_scrollback']} 10k={r['scrollback_10k_lines']}")

print("== scrollback fetch")
for r in [r for r in rows if r["workload"] == "scrollback"]:
    print(f"  {r['shape']:7s} lines={r['lines']} bytes={r['bytes']} round trip {fmt(r['fetch_round_trip'])} us")
