#!/bin/sh
# live.fig's stream, made up: a CSV row every 200 ms (time, rpm, errors) until it's stopped.
# Times count on from when it started, a fifth of a second a row, in UTC. POSIX awk has no
# strftime, so `iso` writes a time from its seconds (Howard Hinnant's civil_from_days).
exec awk -v start="$(date -u +%s)" '
function iso(t,    s, f, z, sec, era, doe, yoe, y, doy, mp, d, m) {
    s = int(t); f = t - s
    z = int(s / 86400); sec = s - z * 86400 + f
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
    return sprintf("%04d-%02d-%02dT%02d:%02d:%04.1fZ", y, m, d, int(sec / 3600), int(sec % 3600 / 60), sec % 60)
}
BEGIN {
    print "time,rpm,errors"
    fflush()
    for (i = 0; ; i++) {
        # A swell every 5 minutes, a ripple every 45 s and a little noise.
        rpm = 1200 + 220 * sin(i / 240) + 70 * sin(i / 36) + (i * 37) % 30
        printf "%s,%d,%d\n", iso(start + i / 5), rpm, ((i * 13) % 17 == 0)
        fflush()
        system("sleep 0.2")
    }
}'
