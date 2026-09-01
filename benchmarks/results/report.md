## Performance Benchmarks

System: Linux 7.0.0-29-generic, CPU: Intel(R) Xeon(R) Gold 6140 CPU @ 2.30GHz
Node: v22.22.1, Rust: undefined
Warmup: 1,000 events, Measured: 100,000 events, Trials: 5
Timestamp: 2026-09-01T07:44:14.075Z

| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup | Actors | Avg Spawned |
|----------|----------------|-------------------|---------|--------|-------------|
| actor-broadcast | 515,983.2      | 645,410.4         | 1.3x    | 3.059 ms / 0.537 ms |
| actor-ping-pong | 698,824.4      | 1,082,573.4       | 1.5x    | 1      | 1           |
| actor-reducer   | 396,390.8      | 430,911.4         | 1.1x    | 0.168 ms / 0.146 ms |
| actor-spawn     | 735,244.4      | 1,060,432.4       | 1.4x    | 1      | 1           |
| actor-tree      | 12,782,853.6   | 1,153,378         | 0.1x    | 2      | 0           |
| auth            | 181,607.4      | 318,012.4         | 1.8x    | 0.271 ms / 0.096 ms |
| counter         | 401,997.4      | 410,839.2         | 1.0x    | 0.140 ms / 0.031 ms |
| entry-exit      | 793,914.8      | 1,086,490         | 1.4x    | 0.178 ms / 0.057 ms |
| form            | 44,598,726.8   | 1,082,882.4       | 0.0x    | 0.152 ms / 0.028 ms |
| stopwatch       | 138,740.2      | 458,413.4         | 3.3x    | 0.190 ms / 0.035 ms |
| timer           | 537,404.2      | 489,360.8         | 0.9x    | 0.184 ms / 0.046 ms |
| toggle          | 205,582.6      | 423,617.8         | 2.1x    | 0.176 ms / 0.047 ms |
| traffic-light   | 201,842.2      | 446,431.8         | 2.2x    | 0.149 ms / 0.030 ms |
| word            | 72,774.6       | 179,416.6         | 2.5x    | 0.412 ms / 0.097 ms |
