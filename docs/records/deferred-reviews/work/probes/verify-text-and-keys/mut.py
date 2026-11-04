import re
import subprocess, sys, os, shutil
S = "/private/tmp/claude-501/-Users-user-workspace-die/80c10c08-831a-470d-a338-74029a942676/scratchpad/verify-text-and-keys"
R = S + "/repo/crates/die-core/"
M = {
 "A": ("src/undo.rs", "let sel = self.pending_open.take().unwrap_or(sel_before);", "let sel = { self.pending_open = None; sel_before };"),
 "C": ("src/undo.rs", "        to.push(Entry::Group(p.other));\n        self.depth = 0;", "        to.push(Entry::Group(p.other));"),
 "E": ("src/text.rs", "            silent: matches!(rec, Rec::Silent),", "            silent: !matches!(rec, Rec::Record(_)),"),
 "F": ("src/text.rs", "        self.log.push_back(change);\n", "        if !matches!(rec, Rec::History) { self.log.push_back(change); }\n"),
 "G": ("src/text.rs", "        self.actor.and_then(|w| self.views.get(&w)).map(|v| v.sel)", "        self.views.values().next().map(|v| v.sel)"),
 "I": ("src/undo.rs", "        head: m(sel.head),\n        ..sel\n    }", "        head: m(sel.head),\n        goal: if shifts.is_empty() { sel.goal } else { None },\n        ..sel\n    }"),
}
for name in sys.argv[1:]:
    f, old, new = M[name]
    p = R + f
    src = open(p).read()
    assert src.count(old) == 1, (name, src.count(old))
    open(p, "w").write(src.replace(old, new))
    try:
        env = dict(os.environ, DIE_PROP_CASES="2000", CARGO_TARGET_DIR=S + "/target-mut")
        r = subprocess.run(["cargo", "test", "-p", os.environ.get("PKG","die-core"), "--offline", "--no-fail-fast", "-q"], cwd=S + "/repo", env=env, capture_output=True, text=True)
        out = r.stdout + r.stderr
        fails = sorted({l.split()[0] for l in out.splitlines() if l.startswith("    ") and "::" not in l and l.strip() and not l.strip().startswith(("left", "right", "thread", "at ", "note"))})
        failed = [l for l in out.splitlines() if "FAILED" in l or "panicked" in l]
        bad=[l for l in out.splitlines() if re.match(r'^test .* FAILED', l)] + [l for l in out.splitlines() if re.match(r'^    \S+$', l)]
        print(name, 'exit', r.returncode, bad[:10]); continue
        print(name, "exit", r.returncode, "| failing tests:", [l for l in out.splitlines() if l.startswith("test ") and "FAILED" in l][:8], [l for l in out.splitlines() if l.startswith("    ") and not ' ' in l.strip()][:8])
        if r.returncode != 0 and "error[" in out:
            print(out[-2000:])
    finally:
        open(p, "w").write(src)
