#!/bin/sh
# What K4's review asked tty Emacs: where the cursor is really drawn on a control character with no
# cell left on its row, and the numbers when a Hide joins lines or covers the last line, or the whole
# text. Private ptys, a temporary HOME.
d=$(cd "$(dirname "$0")" && pwd)
h=$(mktemp -d "$d/home.XXXXXX")
one() {
  LNUM="$1" TEXTC="$2" CURC="$3" COLSC="$4" FOLD="$5" HOME="$h" emacs -Q --batch -l "$d/screen4.el" -f screen-main
}
A=$(printf '\001')
one "" "aaaaaaaaaa${A}b" 10 11 ""
one "" "aaaaaaaaa ${A}b" 10 11 ""
one "" "aaaaaaaa ${A}b" 9 11 ""
one "" "${A}${A}${A}${A}${A}${A}" 5 11 ""
for mode in t relative visual; do
  one "$mode" "a@b" 0 30 "2 3"
  one "$mode" "a@b" 3 30 "2 3"
  one "$mode" "a@b@" 0 30 "2 3"
  one "$mode" "ab" 0 30 "0 2"
  one "$mode" "a@b" 0 30 "0 3"
  one "$mode" "a@b@" 0 30 "0 4"
  one "$mode" "a@b@" 0 30 "3 4"
  one "$mode" "a@b@c@d@" 0 30 "1 4"
  one "$mode" "a@b@c@d@" 4 30 "1 4"
  one "$mode" "a@b@c@d@" 5 30 "1 4"
done
rm -rf "$h"
