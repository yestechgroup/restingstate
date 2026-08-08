# restingstate: A Rust Statechart Compiler and Runtime

| Status | Crate | Crate |
|--------|-------|-------|
| 92 tests pass | 14/16 xstate.js compatibility vectors | `.ts` → parser → IR → codegen → native / WASM / NAPI |

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
└── xstate-napi/        # Node.js bindings + WASM
```

| Crate | Purpose | Depends On |
|-------|---------|------------|
| `xstate-ir` | Language-neutral XState intermediate representation | — |
| `xstate-core` | Pure Rust runtime (Machine trait, Snapshot, StateValue, Interpreter) | `xstate-ir` |
| `xstate-parser` | OXC-based TypeScript parser, extracts XState AST → IR | `xstate-ir` |
| `xstate-codegen` | `quote`/`syn` Rust code generator (IR → Rust source) | `xstate-ir` |
| `xstate-macro` | `#[xstate("path.ts")]` proc macro | `xstate-ir`, `xstate-parser`, `xstate-codegen` |
| `xstate-napi` | Node.js native bindings + WASM target | `xstate-core` |

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

### Runner Usage

```bash
# Generate xstate.js snapshots
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json > js-out.json

# Generate Rust snapshots
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json > rs-out.json

# Compare
node tests/compatibility/compare.mjs js-out.json rs-out.json
```

**Current status: 14/16 test vectors pass** against the xstate.js reference implementation.

The xstate.js source is vendored as a git submodule at `tests/compatibility/xstate-vendor/` (pinned to a specific commit). Test vectors are derived from xstate.js test files without modifying them. Each vector maps a subset of xstate.js behavior that our runtime is expected to match.

---

## 6. Build & Test

```bash
cargo build --workspace                        # Build all crates
cargo test --workspace                         # Run all unit tests (92 tests)
cargo fmt --all -- --check                     # Format check
cargo clippy --all-targets --all-features -- -D warnings  # Lint

# Compatibility harness
cargo run -p rust-runner -- tests/compatibility/machines/simple/toggle.json
node tests/compatibility/ts-runner.mjs tests/compatibility/machines/simple/toggle.json
node tests/compatibility/compare.mjs js-out.json rs-out.json

# WASM build
cargo build -p xstate-napi --target wasm32-unknown-unknown
```

---

## 7. Version Pinning

- **XState v5**: pinned in `packages/package.json`
- **OXC**: 0.77.x
- **Rust**: edition 2021, MSRV TBD (likely 1.75+)
