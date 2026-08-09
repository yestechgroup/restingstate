## Performance Benchmarks

System: Linux 7.0.0-22-generic, CPU: Intel(R) Xeon(R) Gold 6140 CPU @ 2.30GHz
Node: v22.22.1, Rust: rustc 1.96.0 (ac68faa20 2026-05-25)
Warmup: 1,000 events, Measured: 100,000 events, Trials: 5
Timestamp: 2026-08-09T07:47:11.085Z

| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup | Startup (js / wasm) |
|----------|----------------|-------------------|---------|--------------------|
| auth     | 187,543.2      | 140,180.2         | 0.7x    | 1.509 ms / 0.000 ms |
| counter  | 411,561.6      | 170,325.6         | 0.4x    | 0.439 ms / 0.000 ms |
| entry-exit | 809,800.2      | 288,835.6         | 0.4x    | 0.375 ms / 0.000 ms |
| form     | 14,882,352.2   | 247,859.4         | 0.0x    | 0.352 ms / 0.000 ms |
| stopwatch | 135,092.8      | 163,275.4         | 1.2x    | 0.431 ms / 0.000 ms |
| timer    | 536,632.4      | 168,524.8         | 0.3x    | 0.426 ms / 0.000 ms |
| toggle   | 208,757.6      | 198,872.6         | 1.0x    | 0.220 ms / 0.000 ms |
| traffic-light | 201,809.4      | 199,223.2         | 1.0x    | 0.348 ms / 0.000 ms |
| word     | 87,233.8       | 118,692.2         | 1.4x    | 0.808 ms / 0.000 ms |
