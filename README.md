# restingstate: A Rust Statechart Compiler and Runtime

| Status | Crate | Crate |
|--------|-------|-------|
| 205 tests pass | 25 hand-crafted + 34 extracted compatibility vectors | `.ts` → parser → IR → codegen → native / WASM / NAPI |

## 1. Project Overview

**restingstate** is an XState-compatible statechart compiler and runtime for Rust. Rather than building a competing state-machine authoring ecosystem, the project positions itself as a **Rust execution target for XState statecharts**.

The core philosophy is to treat the **XState machine AST** as the true source language, making the fact that it originates in a TypeScript (`.ts`) file an implementation detail. This allows developers to design and visualize state machines using the existing Stately/XState ecosystem, while deploying the execution logic to highly performant, safe, and portable Rust environments.

### Core Objectives

- **Declarative Source:** Use TypeScript/XState definitions as the portable, canonical design artifact.
- **Pure Rust Runtime:** Generate strongly typed Rust enums, structs, and `match` statements. The runtime knows nothing about TypeScript or JavaScript.
- **Portability:** Support Native, WebAssembly (WASM), and embedded execution targets.
- **Interoperability:** Full serialization/deserialization support for persistence, snapshots, and WASM messaging.
- **Compile-Time Safety:** Leverage Rust's macro system to catch missing states, invalid transitions, and unknown actions at compile time rather than runtime.

---

## 2. Architecture & Project Structure

The architecture separates the parsing, intermediate representation (IR), code generation, and runtime phases to ensure extensibility (e.g., adding JSON or SCXML frontends later).

### The Compilation Pipeline

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

### Crate Layout

```
crates/
├── xstate-ir/              # Language-neutral IR (serde)
├── xstate-core/            # Runtime (Machine trait, Interpreter, StateValue, ActorLogic, backend traits)
├── xstate-parser/          # OXC-based TS parser
├── xstate-codegen/         # IR → Rust source (quote/syn)
├── xstate-macro/           # #[xstate("path.ts")] proc macro
├── xstate-napi/            # Node.js bindings (NAPI-RS)
├── xstate-actor/           # Async actor system (Tokio, spawn, send_to, send_parent)
├── xstate-backend-native/  # Single-threaded WASM-compatible actor system
├── xstate-wasm/            # WASM bindings (JsMachine, JsActorSystem, JS callbacks)
└── xstate-bench/           # Criterion benchmarks
```

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
| `xstate-wasm` | WASM bindings (JsMachine, JsActorSystem, JS callbacks) | `xstate-core`, `xstate-backend-native`, `wasm-bindgen` |
| `xstate-bench` | Criterion benchmarks for Interpreter and NativeActorSystem | `xstate-core`, `xstate-backend-native`, `criterion` |

---

## 3. Usage

### A. Proc Macro (compile-time)

The proc macro parses a TypeScript XState file at compile time, generates typed Rust enums/structs, and implements the `Machine` trait — all statically checked.

```rust
use xstate_macro::xstate;

xstate!("machines/toggle.ts");

// This generates:
//   pub enum State { Inactive, Active }
//   pub enum Event { Toggle }
//   pub struct Context { pub count: i64 }
//   pub struct MachineImpl;
//   impl Machine for MachineImpl { ... }

let machine = MachineImpl;
let state = machine.initial_state();
let mut ctx = machine.initial_context();
let new_state = machine.transition(&state, &Event::Toggle, &mut ctx);
```

### B. Dynamic Interpretation (runtime)

Load a machine definition from JSON at runtime and step through it with the `Interpreter`. This path supports guards and actions supplied as closures.

```rust
use xstate_core::Interpreter;
use xstate_ir::Machine;

let machine: Machine = serde_json::from_str(&json)?;
let interpreter = Interpreter::new(machine);
let state = interpreter.initial_state();
let mut ctx = interpreter.initial_context();

let guard = |_: &str, _: &serde_json::Value, _: &serde_json::Value| true;
let mut action_fn = |_: &str, _: &[serde_json::Value], _: &serde_json::Value| {};

let new_state = interpreter.send(&state, &event, &mut ctx, &guard, &mut action_fn, &clock);
```

### C. NAPI-RS (Node.js)

The `xstate-napi` crate exposes a native Node.js binding via NAPI-RS.

```typescript
const { JsMachine } = require('xstate-napi');

const machine = new JsMachine(machineJson);
const snapshot = machine.send({ type: 'TOGGLE' });
// snapshot → { value: "active", context: {} }
```

| Method | Input | Output |
|--------|-------|--------|
| `new JsMachine(json)` | JSON string of xstate-ir Machine | Machine instance |
| `JsMachine.fromIr(json, ctx)` | JSON string, initial context | Machine instance with custom context |
| `.snapshot()` | — | `{ value, context }` |
| `.send(event)` | `{ type: string }` | `{ value, context }` or null |

### D. WASM (Browser)

The `xstate-wasm` crate provides browser-compatible bindings via wasm-bindgen.

```bash
cd crates/xstate-wasm && ./build.sh
```

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

## 4. Conformance

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
| assign built-in | ✓ | 0.1 |
| Always transitions | ✓ | 0.2 |
| History states | ✓ | 0.2 |
| Delayed transitions (after) | ✓ | 0.2 |
| Invoked actors | ✓ | 0.2 |
| Persistence (serialize/restore) | ✓ | 0.2 |
| Spawned actors | ✓ | 0.3 |
| Actor systems | ✓ | 0.3 |
| setup({ actors, guards, actions }) | ✓ | 0.3 |

All 0.1, 0.2, and 0.3 conformance targets are complete. See [docs/CONFORMANCE.md](docs/CONFORMANCE.md) for details.

---

## 5. Compatibility Harness

The compatibility harness runs JSON test vectors through both the XState v5 JavaScript runtime and our Rust runtime, then diffs the resulting snapshots to verify behavioral equivalence.

Test vectors live in `tests/compatibility/machines/` organized by category:

- `simple/` — atomic state machines (toggle, traffic-light)
- `compound/` — nested/compound state machines (form)
- `guards/` — machines with guarded transitions (auth)
- `parallel/` — parallel states (word, flat, nested)
- `final/` — final states (simple, compound, parallel)
- `initial/` — initial state resolution (deep, parallel, compound)
- `actions/` — entry/exit/transition actions (entry-exit, nested, reenter)
- `always/` — eventless (always) transitions
- `after/` — delayed transitions
- `history/` — history states
- `invoke/` — invoked actors
- `payload/` — event payload handling
- `persistence/` — snapshot save/restore

An extraction tool (`tests/compatibility/extract-test-vectors.mjs`) automatically derives additional test vectors from the vendored xstate.js test suite:

```bash
node tests/compatibility/extract-test-vectors.mjs \
  tests/compatibility/xstate-vendor/packages/core/test/history.test.ts
```

### Runner Usage

```bash
# Generate xstate.js snapshots
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json > js-out.json

# Generate Rust snapshots
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json > rs-out.json

# Compare
node tests/compatibility/compare.mjs js-out.json rs-out.json
```

**Current status: 25 hand-crafted + 34 extracted vectors pass** against the xstate.js reference implementation.

The xstate.js source is vendored as a git submodule at `tests/compatibility/xstate-vendor/` (pinned to a specific commit). Test vectors are derived from xstate.js test files without modifying them. Each vector maps a subset of xstate.js behavior that our runtime is expected to match.

---

## 6. Performance Benchmarks

### FSM Workloads (xstate.js vs xstate-wasm)

**Methodology:** 1,000 warmup events, 100,000 measured events, 5 trials, mean throughput. Both runtimes run in the same Node.js process.

| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup |
|----------|----------------|-------------------|---------|
| stopwatch (compound + assign) | 138,740 | **458,413** | **3.3x** |
| word (4-region parallel) | 72,775 | **179,417** | **2.5x** |
| traffic-light (compound) | 201,842 | **446,432** | **2.2x** |
| toggle (atomic) | 205,583 | **423,618** | **2.1x** |
| auth (guards) | 181,607 | **318,012** | **1.8x** |
| entry-exit (actions) | 793,915 | **1,086,490** | **1.4x** |
| counter (assign) | 401,997 | 410,839 | 1.0x |
| timer (always + guard) | 537,404 | 489,361 | 0.9x |

### Actor Workloads (xstate.js vs xstate-wasm)

| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup |
|----------|----------------|-------------------|---------|
| actor-ping-pong | 698,824 | **1,082,573** | **1.5x** |
| actor-spawn | 735,244 | **1,060,432** | **1.4x** |
| actor-broadcast | 515,983 | **645,410** | **1.3x** |
| actor-reducer | 396,391 | 430,911 | 1.1x |

### Rust-native (Criterion, no WASM overhead)

| Benchmark | Time | Throughput |
|-----------|------|------------|
| Interpreter toggle | 331 ns | ~3M events/sec |
| Interpreter guarded | 121 ns | ~8.3M events/sec |
| Interpreter actions | 118 ns | ~8.5M events/sec |
| Actor send 1 event | 645 ns | ~1.5M events/sec |
| Reducer send 1 event | 178 ns | ~5.6M events/sec |
| Actor spawn 1 | 842 ns | ~1.2M actors/sec |

### Running the benchmarks

```bash
# Rust-native (Criterion)
cargo bench -p xstate-bench --bench interpreter
cargo bench -p xstate-bench --bench actor_system

# WASM vs xstate.js
cd benchmarks
npm ci
node run-benchmarks.mjs                         # All workloads
node run-benchmarks.mjs workloads/toggle.json    # Single workload
```

Results are written to `benchmarks/results/report.md`.

---

## 7. Build & Test

```bash
cargo build --workspace                        # Build all crates
cargo test --workspace                         # Run all unit tests (205 tests)
cargo fmt --all -- --check                     # Format check
cargo clippy --all-targets --all-features -- -D warnings  # Lint

# Compatibility harness
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json
node tests/compatibility/compare.mjs js-out.json rs-out.json

# WASM build
cargo build -p xstate-wasm --target wasm32-unknown-unknown
cd crates/xstate-wasm && ./build.sh
```

---

## 8. Version Pinning

- **XState v5**: pinned in `packages/package.json`
- **OXC**: 0.77.x
- **Rust**: edition 2024, MSRV 1.97 (toolchain pinned via `rust-toolchain.toml`)

---

## Releases

Releases are tag-driven: pushing a `v*` tag runs
[`.github/workflows/release.yml`](.github/workflows/release.yml), which builds
the WASM package, publishes crates to crates.io in dependency order, and cuts a
GitHub Release. See [CHANGELOG.md](CHANGELOG.md) and the
[releases page](https://github.com/yestechgroup/restingstate/releases).

**Rust** — depend on a released tag:

```toml
[dependencies]
xstate-core = { git = "https://github.com/yestechgroup/restingstate", tag = "v0.2.0" }
```

**npm** — the WASM build ships as [`@restingstate/wasm`](https://www.npmjs.com/package/@restingstate/wasm):

```bash
npm install @restingstate/wasm
```

## Embedding

Consume restingstate as a dependency — Rust via git tag (Mode A) or WASM/JS via npm/Release artifacts (Mode B). Guide: [docs/embedding.md](docs/embedding.md), with executable demos in [`examples/embed`](examples/embed) (`cargo test -p embed`) and [`examples/js-embed`](examples/js-embed) (`node demo.mjs`) — every documented snippet is executed by CI. API stability: [docs/contracts.md](docs/contracts.md); supported runtime path: [docs/runtime-paths.md](docs/runtime-paths.md).
