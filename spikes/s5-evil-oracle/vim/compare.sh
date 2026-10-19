#!/bin/sh
# Replay every fixture in textbook Vim and list where Evil differs.
#   vim/compare.sh            -> vim/report.jsonl (one line per case)
# Needs /usr/bin/vim (9.x).  A dev aid only; nothing reads the report.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
vim=${VIM_BIN:-/usr/bin/vim}
out=$here/vim/report.jsonl
tmp=$(mktemp -d "${TMPDIR:-/tmp}/evil-oracle-vim.XXXXXX")
: >"$out"
for f in "$here"/fixtures/*.json; do
  t=$tmp/$(basename "$f" .json).jsonl
  "$vim" --not-a-term -u NONE -i NONE -N -n -S "$here/vim/compare.vim" \
    -c "call CompareFixture('$f', '$t')" -c 'qa!' </dev/null >/dev/null 2>&1 || true
  [ -f "$t" ] && cat "$t" >>"$out"
done
rm -rf "$tmp"
echo "vim/compare.sh: $(grep -c '"differs"' "$out") cases compared," \
     "$(grep '"differs"' "$out" | grep -cv '"differs":\[\]' || true) differ," \
     "$(grep -c '"skipped"' "$out" || true) skipped" >&2
