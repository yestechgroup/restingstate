# restingstate: A Rust Statechart Compiler and Runtime

| Status | Crate | Crate |
|--------|-------|-------|
| 128 tests pass | 25 hand-crafted + 36 extracted compatibility vectors | `.ts` → parser → IR → codegen → native / WASM / NAPI |

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
    ├── native
    ├── WASM
    └── NAPI-RS
```

### Crate Layout

```
crates/
├── xstate-ir/          # Language-neutral IR (serde)
├── xstate-core/        # Runtime (Machine trait, Interpreter, StateValue)
├── xstate-parser/      # OXC-based TS parser
├── xstate-codegen/     # IR → Rust source (quote/syn)
├── xstate-macro/       # #[xstate("path.ts")] proc macro
├── xstate-napi/        # Node.js bindings (NAPI-RS)
├── xstate-actor/       # Async actor system (Tokio, supervision, spawn)
└── xstate-wasm/        # WASM bindings (wasm-bindgen)
```

| Crate | Purpose | Depends On |
|-------|---------|------------|
| `xstate-ir` | Language-neutral XState intermediate representation | — |
| `xstate-core` | Pure Rust runtime (Machine trait, Snapshot, StateValue, Interpreter) | `xstate-ir` |
| `xstate-parser` | OXC-based TypeScript parser, extracts XState AST → IR | `xstate-ir` |
| `xstate-codegen` | `quote`/`syn` Rust code generator (IR → Rust source) | `xstate-ir` |
| `xstate-macro` | `#[xstate("path.ts")]` proc macro | `xstate-ir`, `xstate-parser`, `xstate-codegen` |
| `xstate-napi` | Node.js native bindings | `xstate-core` |
| `xstate-actor` | Async actor system (supervision, send_to/send_parent, spawn) | `xstate-core`, `tokio` |
| `xstate-wasm` | WASM bindings for browser/Node.js | `xstate-core`, `wasm-bindgen` |

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

let guard = |_: &str, _: &serde_json::Value| true;
let mut action_fn = |_: &str, _: &serde_json::Value| {};

let new_state = interpreter.send(&state, "TOGGLE", &mut ctx, &guard, &mut action_fn);
```

### C. NAPI-RS (Node.js)

The `xstate-napi` crate exposes a native Node.js binding via NAPI-RS and can also compile to WASM.

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

**Current status: 25 hand-crafted + 36 extracted vectors pass** against the xstate.js reference implementation.

The xstate.js source is vendored as a git submodule at `tests/compatibility/xstate-vendor/` (pinned to a specific commit). Test vectors are derived from xstate.js test files without modifying them. Each vector maps a subset of xstate.js behavior that our runtime is expected to match.

---

## 6. Performance Benchmarks

The benchmark harness (`benchmarks/`) compares xstate.js against the WASM implementation running in-process (via `wasm-bindgen`), using machine definitions extracted from the xstate.js examples.

**Methodology:** 1,000 warmup events, 100,000 measured events, 5 trials, median reporting. Both runtimes run in the same Node.js process.

| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup | Startup (js / wasm) |
|----------|----------------|-------------------|---------|---------------------|
| stopwatch (compound + assign) | 140,117 | **390,742** | **2.8x** | 0.243 ms / 0.129 ms |
| word (4-region parallel) | 87,702 | **196,115** | **2.2x** | 0.464 ms / 0.106 ms |
| traffic-light (compound) | 212,746 | **396,621** | **1.9x** | 0.187 ms / 0.035 ms |
| toggle (atomic) | 217,439 | **386,329** | **1.8x** | 0.149 ms / 0.056 ms |
| auth (guards) | 185,039 | **276,552** | **1.5x** | 1.636 ms / 0.766 ms |
| entry-exit (actions) | 792,063 | **976,169** | **1.2x** | 0.223 ms / 0.073 ms |
| counter (assign) | 419,903 | 340,118 | 0.8x | 0.509 ms / 0.150 ms |
| timer (always + guard) | 529,681 | 419,659 | 0.8x | 0.222 ms / 0.046 ms |

WASM startup is consistently **3–6x faster** than xstate.js (0.035 ms vs 0.149 ms for toggle).

*Note: `counter` and `timer` show slight slowdowns due to context-mutation and guard-evaluation overhead per event. The `form` workload is excluded because its event set doesn't exercise transitions in xstate.js (no-op comparison).*

### Running the benchmarks

```bash
# Build the WASM package
cargo build -p xstate-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target nodejs --out-dir benchmarks/wasm-pkg \
  target/wasm32-unknown-unknown/release/xstate_wasm.wasm

# Run
cd benchmarks && npm install && npm run bench && npm run report
```

Results are written to `benchmarks/results/report.md`.

---

## 7. Build & Test

```bash
cargo build --workspace                        # Build all crates
cargo test --workspace                         # Run all unit tests (128 tests)
cargo fmt --all -- --check                     # Format check
cargo clippy --all-targets --all-features -- -D warnings  # Lint

# Compatibility harness
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json
node tests/compatibility/compare.mjs js-out.json rs-out.json

# WASM build
cargo build -p xstate-wasm --target wasm32-unknown-unknown
```

---

## 8. Version Pinning

- **XState v5**: pinned in `packages/package.json`
- **OXC**: 0.77.x
- **Rust**: edition 2024, MSRV 1.97 (toolchain pinned via `rust-toolchain.toml`)
