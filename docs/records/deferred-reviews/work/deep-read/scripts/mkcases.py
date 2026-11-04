import json, sys

cases = [
    ("undo-cursor-oldest-change", "jAx<Up>y<Esc>u", ["|ab", "cd"]),
    ("insert-down-keeps-goal", "i<Down><Down>X<Esc>", ["abcd|ef", "ab", "abcdef"]),
    ("short-h-keeps-goal", "j5hj", ["abcdef|gh", "abc", "abcdefgh"]),
    ("phantom-0", "ifoo<CR><Esc>0", "|"),
    ("phantom-dollar", "ifoo<CR><Esc>$", "|"),
    ("phantom-caret", "ifoo<CR><Esc>^", "|"),
    ("phantom-u", "ifoo<CR><Esc>ibar<Esc>u", "|"),
    ("phantom-brace", "ifoo<CR><Esc>}", "|"),
    ("3o-esc", "3o<Esc>", ["  |a"]),
    ("2o-esc", "2o<Esc>", ["  |a"]),
    ("count-insert-arrow", "3ifoo<Left>X<Esc>", ["|z"]),
    ("count-A-newline", "2Afoo<CR>bar<Esc>", ["|x"]),
    ("2O-u", "2Ox<Esc>u", ["a", "|b"]),
    ("e-trailing-space", "e", ["ab|c  "]),
    ("3u-one-step", "ix<Esc>3u", ["|abc"]),
    ("bs-at-bob", "i<BS><Esc>", ["|abc"]),
    ("dollar-combining", "$", ["|café"]),
    ("l-combining", "l", ["|éx"]),
    ("a-combining", "aY<Esc>", ["|éx"]),
    ("3A-CR", "3A<CR><Esc>", ["  |a"]),
    ("undefined-key-clears-count", "3gzl", ["|abcdef"]),
    ("tab-past-above-eol", "a<Tab>X<Esc>", ["abc", "abcde|f"]),
    ("cr-w", "w", ["|foo\r", "bar"]),
    ("cr-dollar", "$", ["|foo\r", "bar"]),
    ("t-semicolon-comma", "t,;,", ["|a,b,c,d"]),
    ("undo-arrow-session-cursor", "Ax<Left><Left>y<Esc>u", ["|abc"]),
    ("undo-up-session-cursor-2", "jAx<Up>y<Esc>u", ["|abcd", "ef"]),
]
d = {"topic": sys.argv[2], "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases))
