#!/bin/sh
# usage: run.sh cases.json out.json
d=$(cd "$(dirname "$0")" && pwd)
h=$(mktemp -d "$d/home.XXXXXX")
HOME="$h" PROBE_CASES="$1" PROBE_OUT="$2" emacs -Q --batch -l "$d/probe.el" -f probe-main
rm -rf "$h"
