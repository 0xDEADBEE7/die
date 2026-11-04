# Every oracle case again with the input's final newline removed: where Evil's
# result differs, the empty line after the final newline changed it (rule 1).
import json, os, sys
O, D = sys.argv[1], sys.argv[2]
for f in sorted(os.listdir(O)):
    fx = json.load(open(os.path.join(O, f)))
    cases = []
    for c in fx["cases"]:
        i = c["input"]
        t, b = i["text"], i["cursor"]["byte"]
        if not t.endswith("\n") or i.get("visual") or b >= len(t.encode()):
            continue
        mk = next(m for m in "|§¤‡" if m not in t)
        raw = t.encode()
        s = (raw[:b] + mk.encode() + raw[b:]).decode()[:-1]
        k = {"id": c["id"], "keys": c["keys"], "text": s, "mode": c["mode"]}
        if mk != "|": k["cursor_marker"] = mk
        if i.get("registers"): k["registers"] = i["registers"]
        if i.get("clipboard") is not None: k["clipboard"] = i["clipboard"]
        if i.get("window"): k["window"] = i["window"]
        cases.append(k)
    out = {"topic": fx["topic"], "cases": cases}
    if fx["run"] == "tty": out["run"] = "tty"
    json.dump(out, open(os.path.join(D, f), "w"), ensure_ascii=False)
    print(f, len(cases))
