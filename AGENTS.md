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
- Generated code uses `quote` + `syn`; golden-file tests assert exact output
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
- `xstate!("path/to/machine.ts")` proc macro invocation

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
| Event payload | ✓ | 0.1 |
| Guards (cond) | ✓ | 0.1 |
| Context | ✓ | 0.1 |
| Actions (entry/exit/transition) | ✓ | 0.1 |
| Always transitions | ✓ | 0.2 |
| History states | ✓ | 0.2 |
| Delayed transitions (after) | ✗ | 0.2 |
| Invoked actors | ✗ | 0.2 |
| Spawned actors | ✗ | 0.3 |
| Actor systems | ✗ | 0.3 |
| Persistence (serialize/restore) | ✓ | 0.2 |
| **TOTAL (0.1 targets)** | **9/9** | **0.1** |
| **TOTAL (0.2 targets)** | **2/6** | **0.2** |

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
- `guards/` — machines with guarded transitions (auth)
- `parallel/` — parallel states (word, flat, nested)
- `final/` — final states (simple, compound, parallel)
- `initial/` — initial state resolution (deep, parallel, compound)
- `actions/` — entry/exit/transition actions (entry-exit, nested, reenter)
- `persistence/` — snapshot save/restore (round-trip)
- `always/` — eventless transitions (simple)

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

## Code Generation

### Generated Code Structure

The `xstate-codegen` crate takes an `xstate_ir::Machine` and generates a Rust module containing:

1. **State enum** — one variant per state node (PascalCase)
2. **Event enum** — one variant per unique event name (PascalCase)  
3. **Context struct** — typed fields from ContextSchema
4. **Machine struct** — empty unit struct
5. **impl Machine** — trait impl with match-based transition function

### Example: Toggle Machine

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum State { Inactive, Active }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event { Toggle }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context { pub count: i64 }

pub struct MachineImpl;

impl Machine for MachineImpl {
    type State = State;
    type Event = Event;
    type Context = Context;
    
    fn transition(&self, state: &State, event: &Event, _context: &mut Context) -> State {
        match (state, event) {
            (State::Inactive, Event::Toggle) => State::Active,
            (State::Active, Event::Toggle) => State::Inactive,
            _ => state.clone(),
        }
    }
}
```

### Testing

- Golden-file tests assert exact generated output for known IR inputs
- Compile tests verify generated code compiles and runs correctly against xstate-core

---

## Proc Macro Usage

```
use xstate_macro::xstate;

xstate!("machines/payment.ts");

// This generates:
// - pub enum State { ... }        // All state variants
// - pub enum Event { ... }        // All event variants  
// - pub struct Context { ... }    // Typed context fields
// - pub struct MachineImpl;       // Unit struct
// - impl Machine for MachineImpl; // Full trait implementation

let machine = MachineImpl;
let state = machine.initial_state();
let mut ctx = machine.initial_context();
let new_state = machine.transition(&state, &Event::Submit, &mut ctx);
```

---

## NAPI-RS Bindings

### JsMachine API

The `xstate-napi` crate exposes native Node.js bindings via NAPI-RS:

```typescript
// Node.js usage (via .node addon)
const { JsMachine } = require('xstate-napi');

const machine = new JsMachine(machineJson);
// Returns { value, context }
machine.send({ type: 'TOGGLE' });    // Returns new snapshot or null
```

### API Reference

| Method | Input | Output |
|--------|-------|--------|
| `new JsMachine(json)` | JSON string of xstate-ir Machine | Machine instance |
| `JsMachine.fromIr(json, ctx)` | JSON string, initial context | Machine instance with custom context |
| `.snapshot()` | — | `{ value, context }` |
| `.send(event)` | `{ type: string }` | `{ value, context }` or null |

### WASM Target

The same crate supports a `wasm32-unknown-unknown` build target for WebAssembly deployment:

```bash
cargo build -p xstate-napi --target wasm32-unknown-unknown
```

This enables running XState machines in the browser via WASM, with JSON serialization for state transfer.

---

## Project Status

All 7 planned phases complete. 92 tests pass. 14/16 xstate.js compatibility vectors pass (88%).

---

*Last updated: Phase 0.2 (Persistence + Always transitions + History)*
