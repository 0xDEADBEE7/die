#!/bin/sh
# Spike S2 B(ii): kill -9 of the "DIE" process leaves the helper running; a new DIE process
# reattaches by socket path and finds screen, scrollback (including output produced while no
# DIE existed) and the image intact. No snapshot is involved.
# usage: demo-reattach.sh <s2-binary> <run-dir> [<s2-binary for the new DIE>]
#   With a third argument the helper comes from build N and the new DIE is build N+1.
set -eu
S2=$1
RUN=$2
S2B=${3:-$1}
rm -rf "$RUN"; mkdir -p "$RUN"
"$S2" session --run-dir "$RUN" --phase1 > "$RUN/phase1.json" 2> "$RUN/phase1.err" &
DIE1=$!
i=0
while [ ! -s "$RUN/phase1.json" ] && [ $i -lt 200 ]; do sleep 0.1; i=$((i+1)); done
cat "$RUN/phase1.json"
HELPER=$(sed -n 's/.*"helper_pid":\([0-9]*\).*/\1/p' "$RUN/phase1.json")
SHELL_PID=$(sed -n 's/.*"shell_pid":\([0-9]*\).*/\1/p' "$RUN/phase1.json")
echo "kill -9 DIE process $DIE1 (helper $HELPER, shell $SHELL_PID)"
kill -9 "$DIE1"
wait "$DIE1" 2>/dev/null || true
sleep 0.2
kill -0 "$DIE1" 2>/dev/null && echo "DIE still alive?!" || echo "DIE process is gone"
kill -0 "$HELPER" && echo "helper $HELPER still running"
kill -0 "$SHELL_PID" && echo "shell $SHELL_PID still running"
echo "waiting 4 s with no DIE: the shell's background job prints meanwhile"
sleep 4
echo "new DIE process attaches:"
"$S2B" session --run-dir "$RUN" --phase2
RC=$?
sleep 0.3
kill -0 "$HELPER" 2>/dev/null && echo "helper still running after shutdown?!" || echo "helper exited after Shutdown (cleanup)"
echo "helper log:"; cat "$RUN/rt.log"
exit $RC
