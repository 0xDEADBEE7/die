#!/bin/sh
# live.fig's run, made up: errors a minute for two services over the ten minutes to now, as CSV,
# then it exits. Run again every 5 s, it moves on with the clock.
exec awk -v now="$(date -u +%s)" '
function iso(t,    s, z, sec, era, doe, yoe, y, doy, mp, d, m) {
    s = int(t)
    z = int(s / 86400); sec = s - z * 86400
    z += 719468
    era = int(z / 146097)
    doe = z - era * 146097
    yoe = int((doe - int(doe / 1460) + int(doe / 36524) - int(doe / 146096)) / 365)
    y = yoe + era * 400
    doy = doe - (365 * yoe + int(yoe / 4) - int(yoe / 100))
    mp = int((5 * doy + 2) / 153)
    d = doy - int((153 * mp + 2) / 5) + 1
    m = mp < 10 ? mp + 3 : mp - 9
    if (m <= 2) y++
    return sprintf("%04d-%02d-%02dT%02d:%02d:00Z", y, m, d, int(sec / 3600), int(sec % 3600 / 60))
}
BEGIN {
    print "time,service,errors"
    end = now - now % 60
    for (k = 9; k >= 0; k--) {
        t = end - k * 60
        n = int(t / 60)
        printf "%s,api,%d\n", iso(t), (n * 7) % 5
        printf "%s,worker,%d\n", iso(t), (n * 3) % 4
    }
}'
