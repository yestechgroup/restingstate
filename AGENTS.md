# AGENTS.md — restingstate

## Build & Test Commands

```bash
cargo fmt --all -- --check   # Format check
cargo clippy --all-targets --all-features -- -D warnings   # Lint
cargo build --workspace      # Build all crates
cargo test --workspace       # Run all unit tests
cargo test --workspace -- --ignored  # Run compatibility harness (slow)
```

---

## Crate Map

| Crate | Purpose | Depends On |
|-------|---------|------------|
| `xstate-ir` | Language-neutral XState intermediate representation | — |
| `xstate-core` | Pure Rust runtime (Machine trait, Snapshot, StateValue, Interpreter) | `xstate-ir` |
| `xstate-parser` | OXC-based TypeScript parser, extracts XState AST → IR | `xstate-ir` |
| `xstate-codegen` | `quote`/`syn` Rust code generator (IR → Rust source) | `xstate-ir` |
| `xstate-macro` | `#[xstate("path.ts")]` proc macro | `xstate-ir`, `xstate-parser`, `xstate-codegen` |
| `xstate-napi` | Node.js native bindings + WASM target | `xstate-core` |

---

## Architecture (Compilation Pipeline)

```
machine.ts
    │
    ▼
xstate-parser (OXC)    — Parse TypeScript, extract createMachine(...)
    │
    ▼
xstate-ir              — Language-neutral intermediate representation
    │
    ▼
Validation             — Unreachable states, dead ends, non-determinism, cycles
    │
    ▼
xstate-codegen         — Generate Rust enums, structs, match-based transition fn
    │
    ▼
xstate-core            — Pure Rust runtime (no TS/JS dependency)
    │
    ├── native
    ├── WASM
    └── NAPI-RS
```

---

## Code Conventions

- **Edition 2021**, `resolver = "2"`, `workspace.package` for shared version/license
- Use `workspace.dependencies` for all external crates, local crates use `path = "../…"`
- All public types derive `Debug, Clone, PartialEq, Eq`, and `Serialize, Deserialize` where applicable
- Error types use `thiserror`
- Generated code uses `quote` + `syn`; golden-file tests assert exact output
- Runtime traits: `Machine` has associated types `State`, `Event`, `Context`
- Actions and guards are trait-based boundaries — the generated code stores identifiers, the application supplies implementation
- All IR types are tested with round-trip JSON serialization (`serde_json`)

---

### Supported XState Constructs (Phase 3)

- `createMachine({...})` top-level call
- `id`, `initial`, `context`, `states`
- `on: { EVENT: ... }` with string target, transition object, or array
- `entry` and `exit` action arrays
- Guards (string identifiers)
- Transition actions
- `type` for state types (compound, parallel, final, history)
- `initial` for compound state children
- `setup({}).createMachine({})` chained calls
- `tags` and `description` metadata

---

## IR Schema Reference

- **Machine**: `id` (String), `initial` (String state name), `context` (ContextSchema), `states` (Vec\<StateNode\>)
- **ContextSchema**: `fields` (Vec\<ContextField\>)
- **ContextField**: `name` (String), `type_name` (String), `default_value` (serde_json::Value)
- **StateNode**: `name`, `description` (optional), `initial` (optional child initial state), `entry`/`exit` (Vec\<String\> action names), `transitions` (Vec\<Transition\>), `children` (Vec\<StateNode\>), `node_type` (StateType enum), `tags` (Vec\<String\>)
- **StateType enum**: `Atomic` (default), `Compound`, `Parallel`, `Final`, `History`, `DeepHistory`
- **Transition**: `event` (String), `target` (Option\<String\>), `guard` (Option\<Guard\>), `actions` (Vec\<Action\>), `description` (optional)
- **Guard**: `name` (String) — the identifier of a guard function
- **Action**: `name` (String), `args` (Vec\<serde_json::Value\>)

---

## Parser → IR Mapping

The `xstate-parser` walks the OXC AST to extract `createMachine({...})` calls:

| TypeScript / XState | IR Type | Notes |
|---|---|---|
| `createMachine({ id, initial, ... })` | `Machine` | First argument is the config object |
| `{ states: { ... } }` | `Vec<StateNode>` | Object keys become state names |
| `{ on: { EVENT: "target" } }` | `Vec<Transition>` | Simple string target |
| `{ on: { EVENT: [{ target, guard }] } }` | `Vec<Transition>` | Array of transition objects |
| `{ on: { EVENT: { target, actions } } }` | `Transition` | Single transition object |
| `{ entry: ['action1'] }` | `Vec<String>` entry actions | String array |
| `{ exit: ['action2'] }` | `Vec<String>` exit actions | String array |
| `{ type: 'compound' }` | `StateType::Compound` | State type variant |
| `{ initial: 'childState' }` | `Option<String>` | Optional child initial |
| `{ context: { field: val } }` | `ContextSchema` | Context field definitions |
| `{ tags: ['tag1'] }` | `Vec<String>` | State tags |

---

## Naming

- **state value**: the full tree address of a state node (e.g. `{ red: "walk" }`)
- **state node**: a single node in the machine config with transitions, entry/exit actions
- **snapshot**: `{ state: StateValue, context: C }` — serializable state at a point in time
- **machine**: the compiled statechart structure
- **actor**: a running machine instance with its own snapshot

---

## Conformance Profile

| XState Feature | Status | Target |
|---------------|--------|--------|
| Atomic states | ✓ | 0.1 |
| Compound (nested) states | ✓ | 0.1 |
| Parallel states | ✓ | 0.1 |
| Final states | ✓ | 0.1 |
| Transitions (event → target) | ✓ | 0.1 |
| Event payload | ✗ | 0.1 |
| Guards (cond) | ✓ | 0.1 |
| Context | ✓ | 0.1 |
| Actions (entry/exit/transition) | ✓ | 0.1 |
| Always transitions | ✗ | 0.2 |
| History states | ✗ | 0.2 |
| Delayed transitions (after) | ✗ | 0.2 |
| Invoked actors | ✗ | 0.2 |
| Spawned actors | ✗ | 0.3 |
| Actor systems | ✗ | 0.3 |
| Persistence (serialize/restore) | ✗ | 0.2 |

---

## Compatibility Harness

### Test Vector Format

```json
{
  "machine_name": "toggle",
  "machine_ir": { <xstate-ir Machine> },
  "machine_config": { <xstate.js createMachine config> },
  "protocol": {
    "initial": { "value": "inactive", "context": {} },
    "events": [{ "type": "toggle" }]
  },
  "expected": [
    { "value": "active", "context": {} }
  ]
}
```

### Runner output (per step)

```json
{
  "step": 0,
  "event": { "type": "toggle" },
  "snapshot": {
    "value": "active",
    "context": {}
  }
}
```

### CI flow

```
npm ci (install XState v5)
cargo build --workspace --release
for each test vector:
  ts-runner → xstate-snapshots.json
  rust-runner → rust-snapshots.json
  compare → pass / mismatch report
```

### Test Vectors

Test vectors live in `tests/compatibility/machines/` organized by category:
- `simple/` — atomic state machines (toggle, traffic-light)
- `compound/` — nested/compound state machines (form)
- `guards/` — machines with guarded transitions
- (more categories added as features are implemented)

### Runners

- `tests/compatibility/ts-runner.mjs` — Node.js script that runs test vectors through xstate.js
- `tests/compatibility/rust-runner/` — Rust binary that runs test vectors through the Interpreter
- `tests/compatibility/compare.mjs` — Diffs xstate.js and Rust snapshot outputs

Usage:
```bash
# Generate xstate.js snapshots
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json > js-out.json

# Generate Rust snapshots  
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json > rs-out.json

# Compare
node tests/compatibility/compare.mjs js-out.json rs-out.json
```

The xstate.js source is vendored as a git submodule at `tests/compatibility/xstate-vendor/` (pinned to a specific commit). Test vectors are derived from xstate.js test files without modifying them. Each vector maps a subset of xstate.js behavior that our runtime is expected to match.

---

## Version Pinning

- **XState v5**: pin exact version in `packages/package.json`
- **OXC**: 0.77.x
- **Rust MSRV**: TBD (likely 1.75+)

---

*Last updated: Phase 5 (xstate.js test integration + runners)*
