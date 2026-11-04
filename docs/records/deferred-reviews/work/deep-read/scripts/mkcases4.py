import json, sys

cases = [
    ("w-latin-cyrillic", "w", ["|abcабв def"]),
    ("w-latin-greek", "w", ["|abcαβγ def"]),
    ("w-han-hiragana", "w", ["|日本語です abc"]),
    ("w-hiragana-han", "w", ["|です日本 abc"]),
    ("w-han-katakana", "w", ["|日本テスト abc"]),
    ("w-nbsp", "w", ["|a b c"]),
    ("w-ideographic-space", "w", ["|a　b c"]),
    ("w-emoji", "w", ["|a\U0001F600b c"]),
    ("w-em-dash", "w", ["|a—b c"]),
    ("w-arabic-digits", "w", ["|١٢ abc"]),
    ("w-latin-digits", "w", ["|abc123 def"]),
    ("b-latin-cyrillic", "b", ["abcаб|в def"]),
    ("e-latin-cyrillic", "e", ["|abcабв def"]),
    ("count-ctrl-a", "3<C-a>", ["ab|c", "def", "ghi", "jkl"]),
    ("count-5-close-brace", "5}", ["|a", "", "b"]),
    ("count-5-open-brace", "5{", ["a", "", "|b"]),
    ("count-5w", "5w", ["|foo bar"]),
    ("count-5b", "5b", ["foo |bar"]),
    ("count-5e", "5e", ["|foo bar"]),
    ("count-dollar-past-end-no-final-newline", "5$", "|abc\ndef"),
]
d = {"topic": sys.argv[2], "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases))
