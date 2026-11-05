#!/bin/sh
# Line-number screens in tty Emacs, for tests/emacs_layout: set2's screens.sh, run again with its
# output kept. Each runs a child `emacs -Q -nw` on a private pty, under a temporary HOME.
d=$(cd "$(dirname "$0")" && pwd)
h=$(mktemp -d "$d/home.XXXXXX")
one() {
  LNUM="$1" TEXTC="$2" CURC="$3" HOME="$h" emacs -Q --batch -l "$d/screen3.el" -f screen-main
}
for mode in t relative visual; do
  one "$mode" "" 0
  one "$mode" "one@two@" 8
  one "$mode" "one@two@" 0
  one "$mode" "one@two" 7
  one "$mode" "one@a line that is long enough to wrap around the row@three@" 60
  one "$mode" "one@a line that is long enough to wrap around the row@three@" 40
done
rm -rf "$h"
