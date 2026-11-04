import json, sys
cs = {c["id"]: c for c in json.load(open(sys.argv[1]))}
for o in json.load(open(sys.argv[2])):
    c = cs[o["id"]]
    t = c["text"]
    print(o["id"], "bw", o["body_width"], "starts", o["starts"])
    print("   ", [(i, repr(t[i]) if i < len(t) else "EOB", p) for i, p in enumerate(o["posns"])])
