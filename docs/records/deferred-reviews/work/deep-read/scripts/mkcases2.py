import json, sys

C = "́"
batch = [
    ("o-u-cursor-mid-line", "ox<Esc>u", ["|abc"]),
    ("o-u-cursor-col1", "ox<Esc>u", ["a|bc", "def"]),
    ("O-u-cursor-col1", "Ox<Esc>u", ["abc", "d|ef"]),
    ("A-u-cursor", "Ax<Esc>u", ["a|bc"]),
    ("arrow-before-typing-u", "jA<Up>x<Esc>u", ["|ab", "cd"]),
    ("i-arrow-before-typing-u", "i<Right>x<Esc>u", ["|abc"]),
    ("e-combining-eol", "e", ["|cafe" + C]),
    ("e-combining-eol-2", "e", ["|cafe" + C, "x"]),
    ("w-then-e-combining", "we", ["|a cafe" + C]),
    ("o-u-from-col2-then-esc", "ofoo<CR>bar<Esc>u", ["ab|c"]),
    ("ix-cr-u", "ix<CR>y<Esc>u", ["ab|c"]),
    ("I-u-cursor", "Ix<Esc>u", ["  a|bc"]),
    ("a-u-cursor", "ax<Esc>u", ["a|bc"]),
    ("count-i-u-cursor", "3ix<Esc>u", ["a|bc"]),
    ("count-o-u-cursor", "3ox<Esc>u", ["a|bc"]),
    ("short-l-keeps-goal", "j5lj", ["ab|cdefgh", "abc", "abcdefgh"]),
    ("short-dollar-count", "j9$j", ["abcdef|gh", "abc", "abcdefgh"]),
]
tty = [
    ("tty-insert-down-keeps-goal", "i<Down><Down>X<Esc>", ["abcd|ef", "ab", "abcdef"]),
    ("tty-insert-down-short-line", "i<Down>X<Esc>", ["abcd|ef", "ab", "abcdef"]),
]
which = sys.argv[3]
cases = batch if which == "batch" else tty
d = {"topic": sys.argv[2], "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
if which == "tty":
    d["run"] = "tty"
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases))
