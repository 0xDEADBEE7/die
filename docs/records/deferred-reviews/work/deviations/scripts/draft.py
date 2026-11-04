# Draft hand-written fixtures for the 18 deviations without one, plus the two
# proposed additions, from the Evil fixtures and the Vim runs (scratch only).
import json, re, sys, copy
O, OUT = sys.argv[1], sys.argv[2]
def toks(k): return re.findall(r"<[A-Za-z][-A-Za-z]*>|.", k)
def pos(text, line, col):
    lines = text.split("\n")
    return {"line": line, "col": col, "byte": sum(len(l.encode()) + 1 for l in lines[:line]) + len(lines[line][:col].encode())}
def ch(t): return {"text": t, "type": "char"}
def ln(t): return {"text": t, "type": "line"}
def mac(t): return {"text": t, "type": "macro"}
EOB = [{"symbol": "end-of-buffer", "message": "End of buffer", "after_keys": None}]
R1 = 'Deviation "the last line is the last line" (deviations.json): Vim\'s line. '
spec = {
 "operators/dd-last-line": dict(reason=R1 + "Evil leaves the cursor on the empty line after the final newline.", text="one\ntwo\n", cur=(1,0)),
 "operators/dk": dict(reason=R1 + "The column is the one d started in, as for every linewise d in Evil (S5 row 2).", text="one\n", cur=(0,0)),
 "operators/dG": dict(reason=R1 + "The column is the one d started in, as for every linewise d in Evil (S5 row 2).", text="one\n", cur=(0,0)),
 "geometry/L-short-buffer": dict(reason=R1 + "L stops on the last line.", cur=(3,0), top=0),
 "geometry/ctrl-d-at-end": dict(reason=R1 + "G stops on line 40 and recentres there (top line 34); C-d on the last line fails, as in Vim and Evil.", cur=(39,0), top=34, errors=2),
 "geometry/M-short-buffer": dict(reason=R1 + "M takes the window's lower middle line as Evil does (S5 row 20), clamped to the last line.", cur=(3,0), top=0),
 "geometry/H-at-end": dict(reason=R1 + "G stops on line 40 and recentres there as Evil's does (S5 row 20), so H lands on line 35 (0-based 34).", cur=(34,0), top=34),
 "wrapped/gj-at-last-visual-line": dict(reason=R1 + "gj on the last screen line fails, as in Vim.", cur=(0,0), top=0, errors=2),
 "macros/error-stops-macro": dict(reason=R1 + "j fails on the last line and ends the macro there, one repetition before Evil's.", cur=(2,0),
     regs={'"': ch("c"), "1": ch("c"), "2": ch("b"), "3": ch("a"), "a": mac("xj"), "-": ch("c")}, clip=["a","b","c"], errors=8),
 "macros/recursive": dict(reason=R1 + "j fails on the last line and ends the recursion there.", cur=(3,0),
     regs={'"': ch("d"), "1": ch("d"), "2": ch("c"), "3": ch("b"), "4": ch("a"), "a": mac("xj@a"), "-": ch("d")}, clip=["a","b","c","d"], errors=12),
 "macros/macro-records-count": dict(reason=R1 + "The macro's j fails on the last line: the cursor stays on line 2 (0-based 1), with an error Evil doesn't have.", cur=(1,0), errors=8),
 "search/count-slash": dict(reason='Deviation "a count before / or ? finds the nth match": Vim\'s result.', cur=(0,8)),
 "search/empty-pattern-repeats": dict(reason='Deviation "an empty search repeats the last one": Vim\'s result.', cur=(0,6)),
 "macros/macro-with-dot": dict(reason='Deviation ". inside a macro repeats the macro\'s change": Vim\'s text; registers are Evil\'s kill ring.', text="e f\n", cur=(0,0),
     regs={'"': ch("d "), "1": ch("d "), "2": ch("c "), "3": ch("b "), "4": ch("a "), "a": mac("dw."), "-": ch("d ")}, clip=["a ","b ","c ","d "], errors=0),
 "macros/preset-macro-with-dot": dict(reason='Deviation ". inside a macro repeats the macro\'s change": Vim\'s text; registers are Evil\'s kill ring.', text="c d e f\n", cur=(0,0),
     regs={'"': ch("b "), "1": ch("b "), "2": ch("a "), "-": ch("b ")}, clip=["a ","b "], errors=0),
 "visual/V-p-linewise": dict(reason='Deviation "a linewise put over the last line keeps the final newline": Vim\'s text and cursor; registers are Evil\'s.', text="one\none\n", cur=(1,0)),
 "geometry/3H": dict(reason='Deviation "a count before H counts from 1": Vim\'s line.', cur=(2,0), top=0),
 "undo/ctrl-r-redo": dict(reason='Deviation "undo is Vim\'s u and C-r": C-r redoes the x.', text="bc\n", cur=(0,0), errors=0),
 "undo/u-after-move-redoes": dict(reason='Deviation "undo is Vim\'s u and C-r": the second u undoes the first x.', text="abcd\n", cur=(0,0)),
 "ex/s-groups": dict(reason='Deviation "regular expressions are Rust\'s": the same change in Rust syntax, with Evil\'s result.', keys=":s/(foo) (bar)/$2 $1/<CR>"),
}
files = {}
for name, s in spec.items():
    topic, cid = name.split("/")
    fx = json.load(open(f"{O}/{topic}.json"))
    c = copy.deepcopy(next(x for x in fx["cases"] if x["id"] == cid))
    c.pop("vim", None); c.pop("description", None)
    c["source"] = "hand-written"; c["reason"] = s["reason"]
    if "keys" in s: c["keys"] = s["keys"]; c["key_tokens"] = toks(s["keys"])
    e = c["expected"]
    if "text" in s: e["text"] = s["text"]
    e["cursor"] = pos(e["text"], *s.get("cur", (e["cursor"]["line"], e["cursor"]["col"])))
    if "top" in s: e["window"]["top_line"] = s["top"]
    if "regs" in s: e["registers"] = s["regs"]
    if "clip" in s: e["clipboard"] = {"text": s["clip"][-1], "writes": s["clip"]}
    if "errors" in s:
        if s["errors"]: e["errors"] = [dict(EOB[0], after_keys=s["errors"] if s["errors"] > 2 or name.startswith(("wrapped","geometry")) else s["errors"])]
        else: e.pop("errors", None)
    hdr = files.setdefault(topic, {k: fx[k] for k in ["schema","topic","run","evil_version","evil_source_sha256","emacs_version"]} | {"generator": "hand-written", "cases": []})
    hdr["cases"].append(c)
for topic, f in files.items():
    f = {k: f[k] for k in ["schema","topic","generator","run","evil_version","evil_source_sha256","emacs_version","cases"]}
    with open(f"{OUT}/{topic}.json", "w") as fh:
        fh.write(json.dumps({k: v for k, v in f.items() if k != "cases"}, ensure_ascii=False, separators=(",", ":"))[:-1] + ',"cases":[\n')
        fh.write(",\n".join(json.dumps(c, ensure_ascii=False, separators=(",", ":")) for c in f["cases"]))
        fh.write("\n]}\n")
    print(topic, len(f["cases"]))
