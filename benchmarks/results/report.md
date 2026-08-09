## Performance Benchmarks

System: Linux 7.0.0-22-generic, CPU: Intel(R) Xeon(R) Gold 6140 CPU @ 2.30GHz
Node: v22.22.1, Rust: undefined
Warmup: 1,000 events, Measured: 100,000 events, Trials: 5
Timestamp: 2026-08-09T07:52:58.782Z

| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup | Startup (js / wasm) |
|----------|----------------|-------------------|---------|--------------------|
| auth     | 185,038.6      | 276,552.2         | 1.5x    | 1.636 ms / 0.766 ms |
| counter  | 419,903.2      | 340,118.2         | 0.8x    | 0.509 ms / 0.150 ms |
| entry-exit | 792,063.4      | 976,168.8         | 1.2x    | 0.223 ms / 0.073 ms |
| form     | 17,917,693     | 961,579.4         | 0.1x    | 0.207 ms / 0.037 ms |
| stopwatch | 140,117.4      | 390,741.6         | 2.8x    | 0.243 ms / 0.129 ms |
| timer    | 529,681.2      | 419,659.4         | 0.8x    | 0.222 ms / 0.046 ms |
| toggle   | 217,439        | 386,329.2         | 1.8x    | 0.149 ms / 0.056 ms |
| traffic-light | 212,746.4      | 396,620.6         | 1.9x    | 0.187 ms / 0.035 ms |
| word     | 87,702.4       | 196,115.2         | 2.2x    | 0.464 ms / 0.106 ms |
