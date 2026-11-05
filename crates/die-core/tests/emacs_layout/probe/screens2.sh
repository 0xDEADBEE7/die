#!/bin/sh
# Where the cursor is really drawn, and the numbers a fold gets. Private ptys, a temporary HOME.
d=$(cd "$(dirname "$0")" && pwd)
h=$(mktemp -d "$d/home.XXXXXX")
one() {
  LNUM="$1" TEXTC="$2" CURC="$3" COLSC="$4" FOLD="$5" HOME="$h" emacs -Q --batch -l "$d/screen4.el" -f screen-main
}
one "" "aaaaaaaaa中b" 9 11 ""
one "" "aaaaaaaaa中b" 10 11 ""
one "" "aaaaaaaaaa@b" 10 11 ""
one "" "aaaaaaaaa	b" 9 11 ""
for mode in t relative visual; do
  one "$mode" "a@b@c@d@e@f@" 0 30 "2 7"
done
rm -rf "$h"
