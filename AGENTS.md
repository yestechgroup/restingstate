# AGENTS.md — restingstate

## Build & Test Commands

```bash
cargo fmt --all -- --check   # Format check
cargo clippy --all-targets --all-features -- -D warnings   # Lint
cargo build --workspace      # Build all crates
cargo test --workspace       # Run all unit tests
cargo test --workspace -- --ignored  # Run compatibility harness (slow)
cargo bench -p xstate-bench  # Run Criterion benchmarks
```

---

## Crate Map

| Crate | Purpose | Depends On |
|-------|---------|------------|
| `xstate-ir` | Language-neutral XState intermediate representation | — |
| `xstate-core` | Pure Rust runtime (Machine trait, Snapshot, StateValue, Interpreter, ActorLogic, backend traits) | `xstate-ir` |
| `xstate-parser` | OXC-based TypeScript parser, extracts XState AST → IR | `xstate-ir` |
| `xstate-codegen` | `quote`/`syn` Rust code generator (IR → Rust source) | `xstate-ir` |
| `xstate-macro` | `#[xstate("path.ts")]` proc macro | `xstate-ir`, `xstate-parser`, `xstate-codegen` |
| `xstate-napi` | Node.js native bindings | `xstate-core` |
| `xstate-actor` | Async actor system (Tokio, spawn, send_to, send_parent) | `xstate-core`, `tokio` |
| `xstate-backend-native` | Single-threaded WASM-compatible actor system | `xstate-core` |
| `xstate-wasm` | WASM bindings (JsMachine, JsActorSystem, JS guard/action callbacks) | `xstate-core`, `xstate-backend-native`, `wasm-bindgen` |
| `xstate-bench` | Criterion benchmarks for Interpreter and NativeActorSystem | `xstate-core`, `xstate-backend-native`, `criterion` |

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
    ├── native (xstate-actor, Tokio-based)
    ├── WASM (xstate-backend-native, synchronous)
    └── NAPI-RS (xstate-napi)
```

**Two runtime paths:** the Interpreter (`xstate-core`) is the supported runtime for advanced
semantics and plugin work; codegen / `xstate!` targets simple flat FSMs only — see
[docs/runtime-paths.md](docs/runtime-paths.md).

---

## Code Conventions

- **Edition 2024**, `resolver = "2"`, `workspace.package` for shared version/license
- Use `workspace.dependencies` for all external crates, local crates use `path = "../…"`
- All public types derive `Debug, Clone, PartialEq, Eq`, and `Serialize, Deserialize` where applicable
- Error types use `thiserror`
- Generated code uses `quote` + `syn`; golden-file tests assert exact output
- Runtime traits: `Machine` has associated types `State`, `Event`, `Context`
- Actions and guards are trait-based boundaries — the generated code stores identifiers, the application supplies implementation
- All IR types are tested with round-trip JSON serialization (`serde_json`)
- WASM builds use `--no-default-features` to exclude tokio

---

### Supported XState Constructs

- `createMachine({...})` top-level call
- `id`, `initial`, `context`, `states`
- `on: { EVENT: ... }` with string target, transition object, or array
- `entry` and `exit` action arrays
- Guards (string identifiers)
- Transition actions
- `type` for state types (compound, parallel, final, history)
- `initial` for compound state children
- `setup({ actors, guards, actions, delays }).createMachine({})` chained calls
- `tags` and `description` metadata
- `xstate!("path/to/machine.ts")` proc macro invocation
- Multi-target transitions (cross-compound `#path` absolute targets)
- `assign` built-in action support
- `invoke` with `src`, `id`, `onDone`, `onError`
- `fromPromise`, `fromCallback`, `fromActorLogic`, `fromMachine` actor definitions

---

## IR Schema Reference

- **Machine**: `id` (String), `initial` (String state name), `context` (ContextSchema), `states` (Vec\<StateNode\>), `setup` (Option\<SetupConfig\>)
- **SetupConfig**: `actors` (HashMap\<String, ActorDef\>), `guards` (HashMap\<String, GuardDef\>), `actions` (HashMap\<String, ActionDef\>), `delays` (HashMap\<String, u64\>)
- **ActorDef**: `FromActorLogic { src }`, `FromPromise { src }`, `FromCallback { src }`, `FromMachine { machine_id }`
- **GuardDef**: `name` (String), `params` (Vec\<GuardParam\>)
- **ActionDef**: `name` (String), `params` (Vec\<serde_json::Value\>)
- **ContextSchema**: `fields` (Vec\<ContextField\>)
- **ContextField**: `name` (String), `type_name` (String), `default_value` (serde_json::Value)
- **StateNode**: `name`, `description` (optional), `initial` (optional child initial state), `entry`/`exit` (Vec\<String\> action names), `transitions` (Vec\<Transition\>), `children` (Vec\<StateNode\>), `node_type` (StateType enum), `tags` (Vec\<String\>), `invoke` (Option\<InvokeConfig\>), `on_done` (Vec\<Transition\>), `output` (Option\<Value\>)
- **StateType enum**: `Atomic` (default), `Compound`, `Parallel`, `Final`, `History`, `DeepHistory`
- **Transition**: `event` (String), `target` (Option\<String\>), `guard` (Option\<Guard\>), `actions` (Vec\<Action\>), `description` (optional), `delay_ms` (Option\<u64\>), `targets` (Vec\<String\>)
- **Guard**: `name` (String) — the identifier of a guard function
- **Action**: `name` (String), `args` (Vec\<serde_json::Value\>)
- **InvokeConfig**: `src` (String), `id` (Option\<String\>), `on_done` (Vec\<Transition\>), `on_error` (Vec\<Transition\>)

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
| `setup({ actors: { ... } })` | `SetupConfig.actors` | Actor definitions |
| `setup({ guards: { ... } })` | `SetupConfig.guards` | Guard definitions |
| `setup({ actions: { ... } })` | `SetupConfig.actions` | Action definitions |
| `invoke: { src, id, onDone, onError }` | `InvokeConfig` | Actor invocation |

---

## Actor System

### ActorLogic Trait

The `ActorLogic` trait defines how an actor processes events:

```rust
pub trait ActorLogic: Send + Sync {
    fn name(&self) -> &str;
    fn initial_snapshot(&self) -> serde_json::Value;
    fn receive(&mut self, event: serde_json::Value, snapshot: &mut serde_json::Value) -> Vec<serde_json::Value>;
    fn stop(&mut self) -> Option<serde_json::Value> { None }
}
```

Implementations: `MachineLogic` (wraps Interpreter), `ReducerLogic` (pure reducer), `CallbackLogic` (long-running callback), `PromiseLogic` (one-shot async).

### Backend Traits

Two backend-agnostic traits in `xstate-core::backend`:

- `ActorSystem` — spawn, send_to, stop, snapshot, status, len, is_empty
- `ActorHandle` — send, stop, snapshot, status

### Backend Implementations

| Backend | Crate | Async? | Target |
|---------|-------|--------|--------|
| Tokio (default) | `xstate-actor` | Yes | Native server |
| Native (WASM) | `xstate-backend-native` | No | Browser, WASM |

### Guard/Action Registries

- `GuardRegistry` — named guards with `and()`, `or()`, `not()` combinators
- `ActionRegistry` — named actions with `sequence()`, `parallel()`, `assign_action()` combinators

---

## WASM Support

### Building

```bash
# Build WASM package
cd crates/xstate-wasm && ./build.sh

# Or manually
cargo build -p xstate-wasm --target wasm32-unknown-unknown --release
wasm-pack build crates/xstate-wasm --target web
```

### JavaScript API

```javascript
import init, { JsMachine, JsActorSystem, registerGuard, registerAction } from '@restingstate/wasm';
await init();

// Single machine
const machine = new JsMachine(JSON.stringify(machineIr));
machine.send(JSON.stringify({ type: 'TOGGLE' }));
console.log(JSON.parse(machine.snapshot()));

// Actor system
const system = new JsActorSystem();
system.spawn("worker", JSON.stringify(workerIr));
system.sendTo("worker", JSON.stringify({ type: "PROCESS" }));

// JS guard/action callbacks
registerGuard("isValid", (ctx, event) => ctx.count > 0);
registerAction("log", (args, event) => console.log(event.type));
```

---

## Benchmarks

### Rust-native (Criterion)

```bash
cargo bench -p xstate-bench --bench interpreter    # Interpreter benchmarks
cargo bench -p xstate-bench --bench actor_system   # Actor system benchmarks
```

### WASM vs xstate.js

```bash
cd benchmarks
node run-benchmarks.mjs                           # All workloads
node run-benchmarks.mjs workloads/toggle.json      # Single workload
```

### Workloads

| Category | Workloads |
|----------|-----------|
| FSM | toggle, traffic-light, auth, counter, entry-exit, form, stopwatch, timer, word |
| Actor | actor-spawn, actor-ping-pong, actor-reducer, actor-broadcast, actor-tree |

---

## Naming

- **state value**: the full tree address of a state node (e.g. `{ red: "walk" }`)
- **state node**: a single node in the machine config with transitions, entry/exit actions
- **snapshot**: `{ state: StateValue, context: C }` — serializable state at a point in time
- **machine**: the compiled statechart structure
- **actor**: a running machine instance with its own snapshot

---

## Conformance Profile

Path legend: **I** = Interpreter path, **C** = codegen / `#[xstate]` path, **B** = both.
See [docs/runtime-paths.md](docs/runtime-paths.md) for the full runtime-path decision.

| XState Feature | Status | Target | Path |
|---------------|--------|--------|------|
| Atomic states | ✓ | 0.1 | B |
| Compound (nested) states | ✓ | 0.1 | I |
| Parallel states | ✓ | 0.1 | I |
| Final states | ✓ | 0.1 | I |
| Transitions (event → target) | ✓ | 0.1 | B |
| Event payload | ✓ | 0.1 | I |
| Guards (cond) | ✓ | 0.1 | I |
| Context | ✓ | 0.1 | B |
| Actions (entry/exit/transition) | ✓ | 0.1 | I |
| assign built-in | ✓ | 0.1 | I |
| Always transitions | ✓ | 0.2 | I |
| History states | ✓ | 0.2 | I |
| Delayed transitions (after) | ✓ | 0.2 | I |
| Invoked actors | ✓ | 0.2 | I |
| Spawned actors | ✓ | 0.3 | I |
| Actor systems | ✓ | 0.3 | I |
| Persistence (serialize/restore) | ✓ | 0.2 | I |
| setup({ actors, guards, actions }) | ✓ | 0.3 | I |
| **TOTAL (0.1 targets)** | **10/10** | **0.1** | |
| **TOTAL (0.2 targets)** | **5/5** | **0.2** | |
| **TOTAL (0.3 targets)** | **3/3** | **0.3** |

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
    "events": [
      { "type": "toggle" },
      { "type": "TICK", "advance_ms": 1100 }
    ]
  },
  "expected": [
    { "value": "active", "context": {} }
  ]
}
```

An optional `"advance_ms": <u64>` on a protocol event advances time by that many ms
before the event is dispatched: ts-runner sleeps with real timers (xstate.js `after`
delays elapse) and rust-runner advances its `MockClock` by the same amount.

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

Test vectors live in `tests/compatibility/machines/` organized by category (25 hand-crafted vectors across 13 categories):
- `simple/` — atomic state machines (toggle, traffic-light)
- `compound/` — nested/compound state machines (form)
- `guards/` — machines with guarded transitions (auth)
- `parallel/` — parallel states (word, flat, nested)
- `final/` — final states (simple, compound, parallel)
- `initial/` — initial state resolution (deep, parallel, compound)
- `actions/` — entry/exit/transition actions (entry-exit, nested, reenter)
- `persistence/` — snapshot save/restore (round-trip)
- `always/` — eventless transitions (simple)
- `after/` — delayed transitions
- `history/` — history states
- `invoke/` — invoked actors
- `payload/` — event payload support

Additional extracted test vectors (31 vectors; the extractor emits only faithfulness-guarded vectors — config/IR transition surface and declared-initial are verified against real xstate.js, artifact vectors are skipped) are generated from the xstate.js
test suite and stored in `tests/compatibility/vectors/extracted/`. Use the
extraction tool to regenerate them:

```bash
node tests/compatibility/extract-test-vectors.mjs \
  tests/compatibility/xstate-vendor/packages/core/test/<name>.test.ts
```

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
- **Rust MSRV**: 1.97 (edition 2024, toolchain pinned via `rust-toolchain.toml`)

---

## Code Generation

### Generated Code Structure

The `xstate-codegen` crate takes an `xstate_ir::Machine` and generates a Rust module containing:

1. **State enum** — one variant per state node (PascalCase)
2. **Event enum** — one variant per unique event name (PascalCase)
3. **Context struct** — typed fields from ContextSchema
4. **Machine struct** — empty unit struct
5. **impl Machine** — trait impl with match-based transition function
6. **Guard module** — placeholder functions (when setup has guards)
7. **Action module** — placeholder functions (when setup has actions)
8. **Actor module** — placeholder factory functions (when setup has actors)

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

---

## Project Status

All planned phases complete. 350+ tests pass. 25 hand-crafted compatibility vectors + 31 faithfulness-guarded extracted vectors. Actor system with dual backends (Tokio + WASM). WASM browser support with full JS API.

---

*Last updated: Phase 1.6 (Actor benchmarks + WASM support)*
