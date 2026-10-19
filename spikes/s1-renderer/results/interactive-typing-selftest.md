# Interactive typing (check 1, real keys)

| phase | keys | p50 ms | p99 ms | max ms |
|---|---|---|---|---|
| no producer | 20 | 22.85 | 30.36 | 30.36 |
| 60 Hz producer | 20 | 26.70 | 34.23 | 34.23 |

Target: p50 ≤ 12 ms, p99 ≤ 17 ms, p99 rise with producer ≤ 1 ms.
Result: p50 FAIL , p99 FAIL , rise 3.87 ms FAIL.
