#!/bin/sh
# emacs -Q --batch with HOME in scratch, and the ~/.emacs.d guard run-oracle.sh uses.
set -eu
real_home=$HOME
scratch=/private/tmp/claude-501/-Users-user-workspace-die/80c10c08-831a-470d-a338-74029a942676/scratchpad/vimwork/ehome
mkdir -p "$scratch"
stamp=$scratch/.stamp
touch "$stamp"
HOME=$scratch emacs -Q --batch "$@" </dev/null
changed=$(find "$real_home/.emacs.d" -newer "$stamp" 2>/dev/null | head -5)
if [ -n "$changed" ]; then
  echo "emacs-batch.sh: ~/.emacs.d changed:" >&2
  echo "$changed" >&2
  exit 3
fi
