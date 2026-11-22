#!/bin/sh
# A source that fails, for the error figure's log: 25 lines on stderr (the log keeps the last
# 20), a header and no rows on stdout, and exit status 3.
i=1
while [ "$i" -le 25 ]; do
    echo "fail.sh: step $i of 25: the upstream said no" >&2
    i=$((i + 1))
done
echo "time,errors"
exit 3
