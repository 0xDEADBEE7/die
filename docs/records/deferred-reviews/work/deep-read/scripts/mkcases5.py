import json, sys

samples = {
    "cyrillic": "а", "greek": "α", "armenian": "ա", "hebrew": "א",
    "arabic": "ب", "thai": "ก", "devanagari": "क", "georgian": "ა",
    "phonetic": "ɐ", "latin-ext": "ŝ", "han": "日", "hangul": "한",
    "fullwidth-latin": "ａ", "greek-upper": "Α",
}
cases = []
for name, c in samples.items():
    cases.append(("w-latin-" + name, "w", ["|abc" + c * 3 + " def"]))
    cases.append(("w-" + name + "-latin", "w", ["|" + c * 3 + "abc def"]))
d = {"topic": sys.argv[2], "cases": [{"id": i, "keys": k, "text": t} for i, k, t in cases]}
with open(sys.argv[1], "w") as f:
    json.dump(d, f, ensure_ascii=False, indent=1)
print(len(cases))
