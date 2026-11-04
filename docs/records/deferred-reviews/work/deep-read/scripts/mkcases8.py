import json, sys

long = " ".join(["word%02d" % i for i in range(24)])  # 167 chars, wraps in an 80-column tty
cases = [
    ("tty-ctrl-a-wrapped", "$<C-a>", ["|" + long, "next"]),
    ("tty-insert-down-wrapped-long", "i<Down>X<Esc>", ["|" + long, "next"]),
]
d = {"topic": sys.argv[2], "run": "tty", "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases), len(long))
