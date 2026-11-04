import json, sys

long = "aaaa bbbb cccc dddd eeee ffff gggg hhhh iiii jjjj kkkk llll mmmm"
cases = [
    ("tty-insert-down-wrapped", "i<Down>X<Esc>", ["|" + long, "next"]),
    ("tty-insert-up-wrapped", "A<Up>X<Esc>", [long, "next|"]),
]
d = {"topic": sys.argv[2], "run": "tty", "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases))
