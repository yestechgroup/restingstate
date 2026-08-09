# XState Conformance

restingstate targets [xstate v5](https://github.com/statelyai/xstate) and
continuously tests compatibility against 25 hand-crafted test vectors (13
categories) plus 34 extracted vectors from the xstate.js test suite. All 0.1,
0.2, and 0.3 targets are complete. Snapshots from both runtimes are compared
automatically in CI.

## Conformance Table

| Feature | Status | Notes |
|---------|--------|-------|
| Atomic states | ✓ | |
| Compound (nested) states | ✓ | nesting, initial child resolution |
| Parallel states | ✓ | multi-region, simultaneous transitions |
| Final states | ✓ | terminal, parent transitions still fire |
| Transitions (event → target) | ✓ | string targets, multi-target, #path absolute |
| Guards (cond) | ✓ | string identifiers, context-key evaluation |
| Context | ✓ | typed fields, JSON serialization |
| Actions (entry/exit/transition) | ✓ | identifier storage, trait-based execution |
| assign built-in | ✓ | built-in action support |
| Event payload | ✓ | typed payload on event objects |
| Always transitions | ✓ | eventless transitions |
| History states | ✓ | shallow and deep history |
| Delayed transitions (after) | ✓ | timer-based transitions |
| Invoked actors | ✓ | child actors via invoke |
| Spawned actors | ✓ | actor system spawn support |
| Actor systems | ✓ | system-level actor management |
| Persistence (serialize/restore) | ✓ | round-trip snapshot save/restore |

**Pass rate:** all 0.1, 0.2, and 0.3 targets complete.

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
| persistence | `persistence/` | snapshot save/restore |
| always | `always/` | eventless transitions |
| after | `after/` | delayed transitions |
| history | `history/` | shallow and deep history |
| invoke | `invoke/` | invoked child actors |
| payload | `payload/` | event payload support |

Additionally, 34 extracted test vectors are generated from the xstate.js test
suite and stored in `tests/compatibility/vectors/extracted/`. Regenerate them:

```bash
node tests/compatibility/extract-test-vectors.mjs \
  tests/compatibility/xstate-vendor/packages/core/test/<name>.test.ts
```

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
