import json, sys

cases = [
    ("j-after-carriage-return", "j", ["a\rb|c", "xyzwv"]),
    ("j-after-control-a", "j", ["a\x01b|c", "xyzwv"]),
    ("k-into-carriage-return", "k", ["a\rbc", "x|yzwv"]),
]
d = {"topic": sys.argv[2], "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases))
