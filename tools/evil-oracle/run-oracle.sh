#!/bin/sh
# Run the Evil oracle: cases/*.json in, fixtures/*.json out.
#
#   ./run-oracle.sh                     all cases -> fixtures/
#   ./run-oracle.sh cases/motions.json  one topic
#   ./run-oracle.sh --reverse --out DIR run cases in reverse order
#
# Environment (all optional):
#   EVIL_ORACLE_EMACS     Emacs binary     (default: emacs on PATH)
#   EVIL_ORACLE_EVIL_DIR  Evil sources     (default ~/.emacs.d/lisp/evil)
#   EVIL_ORACLE_HOME      scratch HOME     (default: a temp directory,
#                                           removed afterwards)
#   EVIL_ORACLE_RUN       batch|tty: run every case file in one mode
#
# Emacs runs as `emacs -Q --batch' (and, for "run": "tty" case files, a
# child `emacs -Q -nw' on a pty) with HOME pointed at the scratch
# directory, so nothing it does can land in ~/.emacs.d.  The script still
# checks afterwards and fails if anything there is newer than the run, or
# if Emacs byte- or native-compiled anything into the scratch HOME.
# (A running Emacs of yours that saves history there would trip it too.)
set -eu

here=$(cd "$(dirname "$0")" && pwd)
real_home=$HOME
emacs=${EVIL_ORACLE_EMACS:-$(command -v emacs || true)}
if [ -z "$emacs" ]; then
  echo "run-oracle.sh: no emacs on PATH; set EVIL_ORACLE_EMACS to your Emacs binary" >&2
  exit 2
fi
evil_dir=${EVIL_ORACLE_EVIL_DIR:-$real_home/.emacs.d/lisp/evil}
if [ -n "${EVIL_ORACLE_HOME:-}" ]; then
  scratch=$EVIL_ORACLE_HOME
else
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/evil-oracle.XXXXXX")
  trap 'rm -rf "$scratch"' EXIT
fi
out=$here/fixtures
order=forward

while [ $# -gt 0 ]; do
  case $1 in
    --reverse) order=reverse; shift ;;
    --out) out=$2; shift 2 ;;
    --) shift; break ;;
    -*) echo "run-oracle.sh: unknown option $1" >&2; exit 2 ;;
    *) break ;;
  esac
done
[ $# -gt 0 ] || set -- "$here"/cases/*.json

mkdir -p "$scratch" "$out"
stamp=$scratch/.evil-oracle-stamp
touch "$stamp"

export HOME="$scratch" EVIL_ORACLE_EVIL_DIR="$evil_dir" EVIL_ORACLE_OUT="$out" \
       EVIL_ORACLE_ORDER="$order"

# Batch Emacs runs every case file without "run": "tty", then starts a
# child `emacs -nw' on a pty for the tty files, which need a real
# redisplay (window geometry, wrapped lines).
EVIL_ORACLE_EMACS=$emacs \
"$emacs" -Q --batch -L "$here/oracle" -l evil-oracle -f evil-oracle-main "$@" \
  </dev/null >/dev/null 2>"$scratch/oracle.log" || {
  echo "run-oracle.sh: Emacs failed; log follows" >&2
  tail -20 "$scratch/oracle.log" >&2
  exit 1
}
grep 'evil-oracle:' "$scratch/oracle.log" >&2 || true

compiled=$(find "$scratch" -newer "$stamp" \( -name '*.eln*' -o -name '*.elc' \) | head -5)
if [ -n "$compiled" ]; then
  echo "run-oracle.sh: Emacs compiled files, which it must not do:" >&2
  echo "$compiled" >&2
  exit 3
fi

if [ -d "$real_home/.emacs.d" ]; then
  changed=$(find "$real_home/.emacs.d" -newer "$stamp" 2>/dev/null | head -20)
  if [ -n "$changed" ]; then
    echo "run-oracle.sh: files under ~/.emacs.d changed during the run:" >&2
    echo "$changed" >&2
    exit 3
  fi
fi
