# XState Conformance

restingstate targets [xstate v5](https://github.com/statelyai/xstate) and
continuously tests compatibility against 16 test vectors derived from the
xstate.js test suite. Snapshots from both runtimes are compared automatically
in CI.

## Conformance Table

| Feature | Status | Notes |
|---------|--------|-------|
| Atomic states | ✓ | |
| Compound (nested) states | ✓ | nesting, initial child resolution |
| Parallel states | ✓ | multi-region, simultaneous transitions |
| Final states | ✓ | terminal, parent transitions still fire |
| Transitions (event → target) | ✓ | string targets |
| Guards (cond) | ✓ | string identifiers, context-key evaluation |
| Context | ✓ | typed fields, JSON serialization |
| Actions (entry/exit/transition) | ✓ | identifier storage, trait-based execution |
| Event payload | ✗ | only `{ type: string }` events currently |
| Always transitions | ✗ | planned 0.2 |
| History states | ✗ | planned 0.2 |
| Delayed transitions (after) | ✗ | planned 0.2 |
| Invoked actors | ✗ | planned 0.2 |
| Spawned actors | ✗ | planned 0.3 |
| Actor systems | ✗ | planned 0.3 |
| Persistence (serialize/restore) | ✗ | planned 0.2 |

**Pass rate:** 14/16 vectors (88%).

## Compatibility Testing

Test vectors live in [`tests/compatibility/machines/`](../tests/compatibility/machines/),
organized by category:

| Category | Directory | Description |
|----------|-----------|-------------|
| simple | `simple/` | toggle, traffic-light |
| compound | `compound/` | nested state machines |
| guards | `guards/` | guarded transitions |
| parallel | `parallel/` | multi-region parallel states |
| final | `final/` | terminal states |
| initial | `initial/` | initial state resolution |
| actions | `actions/` | entry, exit, transition actions |

Each test vector is a JSON file containing a machine IR, a machine config for
xstate.js, and a protocol of events. Two runners — one TypeScript
([`ts-runner.mjs`](../tests/compatibility/ts-runner.mjs)) and one Rust
([`rust-runner/`](../tests/compatibility/rust-runner/)) — replay the same
events through both runtimes and produce snapshots. A comparison script
([`compare.mjs`](../tests/compatibility/compare.mjs)) diffs the outputs.

To run locally:

```bash
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json > js-out.json
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json > rs-out.json
node tests/compatibility/compare.mjs js-out.json rs-out.json
```

## Known Limitations

- **Leaf atomic states in parallel regions** serialize as state name strings.
  xstate.js uses `undefined` (JSON: `{}`) for leaf atomic states within
  parallel nodes.
- **Root parallel states** omit the parallel node name from the state value.
  xstate.js includes it (e.g. `{ red: "walk" }`).
- **Cross-compound transitions** require `#path` absolute target syntax
  (e.g. `"#machineId.state.child"`) when the target is in a different
  compound branch.

## Version Matrix

| Component | Version |
|-----------|---------|
| xstate.js (target) | v5 |
| OXC parser | 0.77.x |
| Rust MSRV | 1.75+ |
