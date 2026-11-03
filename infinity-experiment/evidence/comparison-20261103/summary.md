| label | exit | wall ms | peak RSS KiB | processes | threads | sampled CPU ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| factory-core-recovery-e2e | 0 | 364.449 | 43920 | 2 | 22 | 240.0 |
| factory-parallel-workgraph | 0 | 752.999 | 113332 | 7 | 61 | 390.0 |
| prototype-1 | 0 | 407.773 | 47424 | 3 | 10 | 370.0 |
| prototype-2 | 0 | 370.3 | 50384 | 3 | 10 | 290.0 |
| prototype-3 | 0 | 369.231 | 51608 | 3 | 11 | 290.0 |
| prototype-4 | 0 | 354.946 | 52528 | 3 | 11 | 300.0 |
| prototype-5 | 0 | 350.378 | 52284 | 3 | 10 | 310.0 |
| prototype-restart-recovery | 0 | 360.421 | 49500 | 4 | 12 | 300.0 |

| prototype | success | wall ms | first child ms | callback queue ms | effect→callback ms | duplicate effects | events |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| prototype-1 | True | 379 | 46 | 0.6666666666666666 | 1.3333333333333333 | 0 | 83 |
| prototype-2 | True | 345 | 40 | 0.6666666666666666 | 1.0 | 0 | 81 |
| prototype-3 | True | 344 | 37 | 1.0 | 1.3333333333333333 | 0 | 84 |
| prototype-4 | True | 344 | 36 | 0.6666666666666666 | 1.3333333333333333 | 0 | 84 |
| prototype-5 | True | 340 | 36 | 0.3333333333333333 | 1.3333333333333333 | 0 | 84 |
| prototype-restart | True | 37 | 10 | 1.0 | 1.3333333333333333 | 0 | 25 |
