import json, re, sys
def pos(text, line, col):
    lines = text.split("\n")
    b = sum(len(l.encode()) + 1 for l in lines[:line]) + len(lines[line][:col].encode())
    return {"line": line, "col": col, "byte": b}
def toks(k):
    return re.findall(r"<[A-Za-z][-A-Za-z]*>|.", k)
cases = json.load(open(sys.argv[1]))
out = []
for c in cases:
    out.append({"id": c["id"], "source": "hand-written", "reason": "probe", "keys": c["keys"],
      "key_tokens": toks(c["keys"]), "mode": "fundamental-mode",
      "input": {"text": c["text"], "cursor": pos(c["text"], *c["at"])},
      "expected": {"text": c.get("out", c["text"]), "cursor": pos(c.get("out", c["text"]), *c["want"]),
        "state": "normal", "registers": c.get("regs", {"\"": None}),
        "clipboard": {"text": None, "writes": []},
        **({"errors": [{"symbol": "x", "message": "x", "after_keys": len(toks(c["keys"]))}] * c["errors"]} if c.get("errors") else {})}})
hdr = {"schema": 1, "topic": "probe", "generator": "probe", "run": "batch", "evil_version": "1.15.0",
       "evil_source_sha256": "x", "emacs_version": "30.2", "cases": out}
json.dump(hdr, open(sys.argv[2], "w"), ensure_ascii=False)
