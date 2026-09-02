# Embedding restingstate

**Audience:** projects consuming `restingstate` as a dependency (Rust or JS/WASM).
**Issue:** [#33](https://github.com/yestechgroup/restingstate/issues/33) (dependency-readiness epic #26).
Every snippet below is executed by a demo that runs in CI — links over duplication:

- **Mode A (Rust):** [`examples/embed`](../examples/embed) — `cargo test -p embed` (also `cargo run -p embed`).
- **Mode B (JS/WASM):** [`examples/js-embed`](../examples/js-embed) — `node examples/js-embed/demo.mjs`.

Stability rules live in [contracts.md](contracts.md); the supported-runtime decision
(Interpreter path, not codegen) is [runtime-paths.md](runtime-paths.md). Both modes
below use only Interpreter-path APIs.

| Mode | Artifact | Good for |
|---|---|---|
| **A: Rust** | `xstate-core` + `xstate-parser` + `xstate-backend-native` via Cargo git tag | Native Rust hosts, embedded/edge targets |
| **B: JS/WASM** | `@yestechgroup/wasm` npm package (or the wasm pkg from Release assets) | Node and browser hosts, JS/TS apps |

---

## Mode A: Rust (git-tag dependency)

### Dependency snippet

```toml
[dependencies]
xstate-core           = { git = "https://github.com/yestechgroup/restingstate", tag = "v0.2.0" }
xstate-ir             = { git = "https://github.com/yestechgroup/restingstate", tag = "v0.2.0" }
xstate-parser         = { git = "https://github.com/yestechgroup/restingstate", tag = "v0.2.0" }
xstate-backend-native = { git = "https://github.com/yestechgroup/restingstate", tag = "v0.2.0" }
```

- **MSRV:** 1.97 (`rust-version = "1.97"`; an MSRV bump is a breaking change — see
  [contracts.md](contracts.md)). Tags `vX.Y.Z` are cut per the workspace version.
- **Feature-flag rule for WASM targets:** the `async` default feature pulls tokio.
  When compiling for `wasm32-unknown-unknown`, disable it:
  `xstate-core = { ..., default-features = false }` (and likewise
  `xstate-backend-native = { ..., default-features = false }`). The native backend
  is synchronous and needs no tokio; CI asserts this exact combination
  (`cargo build -p xstate-core --no-default-features --target wasm32-unknown-unknown`).

### End-to-end walkthrough

The executable version of this section is
[`examples/embed/src/lib.rs`](../examples/embed/src/lib.rs) (`pub fn run()` with
assertions on every step; wired as a workspace test in
[`examples/embed/tests/run.rs`](../examples/embed/tests/run.rs)). The sequence:

**1. Parse a machine from TypeScript** (`xstate_parser::parse`, full source in the demo's
`WALLET_TS`):

```rust
let machine: Machine = xstate_parser::parse(WALLET_TS)?;
```

**2. Validate the machine-IR JSON gate** — IR JSON is the pinned interchange format
([contracts.md](contracts.md#machine-ir-schema--validation)); validate before spawning,
and treat failures as host errors:

```rust
let machine_json = serde_json::to_string(&machine)?;
xstate_core::validate_machine_json(&machine_json)?;   // Err on structurally broken IR
```

**3. Build an `Interpreter` with `GuardRegistry`/`ActionRegistry`** — guards are
`(context, event) -> bool`, actions are `(args, event, &mut context)`; a **missing guard
allows** and a **missing action is a no-op** (pinned, [contracts.md](contracts.md#guardregistry--actionregistry)):

```rust
let interpreter = Interpreter::new(machine.clone());
let mut guards = GuardRegistry::new();
guards.register(GuardImpl::new("canSpend", |ctx, event| {
    ctx["balance"].as_i64().unwrap_or(0) >= event["cost"].as_i64().unwrap_or(i64::MAX)
}));
let mut actions = ActionRegistry::new();
actions.register(ActionImpl::new("recordSpend", |_args, event, ctx| {
    let cost = event["cost"].as_i64().unwrap_or(0);
    let spent = ctx["spent"].as_i64().unwrap_or(0);
    ctx["spent"] = json!(spent + cost);
}));
```

**4. Send events, assert snapshots** — `send_with_registry` returns the new `StateValue`
(`None` when nothing matched) and mutates context in place (including `assign`, which the
parser lowers into the `$assign_ops` envelope):

```rust
let clock = MockClock::new(0);
let mut state = interpreter.initial_state();
let mut ctx = interpreter.initial_context();
state = interpreter
    .send_with_registry(&state, &json!({"type": "SPEND", "cost": 25}), &mut ctx,
                        &guards, &actions, &clock)
    .expect("fall-through transition");
assert_eq!(interpreter.snapshot(&state, &ctx),
           json!({"value": "denied", "context": {"balance": 10, "spent": 0}}));
```

(Prefer raw closures over registries? `Interpreter::send` takes
`guards: &dyn Fn(&str, &Value, &Value) -> bool` and `actions: &mut dyn FnMut(&str, &[Value], &Value)`.)

**5. Persist / restore** — `persisted_snapshot` emits the versioned envelope
(`value`, `context`, `history`, `entry_times`, `version: 1`; evolution rules in
[contracts.md](contracts.md#persisted-snapshot-format)); `restore_persisted` validates the
state against the machine before accepting it:

```rust
let persisted = interpreter.persisted_snapshot(&state, &ctx);
let restorer = Interpreter::new(machine.clone());
let (state, ctx) = restorer.restore_persisted(&persisted)?;
// the restored pair keeps processing events as-is
```

**6. Invoke parent/child through `NativeActorSystem`** — entering a state with `invoke`
auto-spawns the child as `{parent}.{src}`; when the child hits a final state it is marked
`Done` and the parent receives `done.invoke.{child_id}`
([pinned strings](contracts.md#stringly-typed-conventions-pinned)). Guards/actions reach
spawned machine actors through `set_registry_factory`:

```rust
let system = NativeActorSystem::new();
system.register_machine("job", job);            // the invoked machine's src: "job"
system.set_registry_factory(move || (guards, actions));   // per-actor registries
let parent = system.spawn_machine("runner", runner);
parent.send(json!({"type": "LOAD"}));           // -> "loading", spawns "runner.job"
assert_eq!(system.get_status("runner.job"), Some(ActorStatus::Done));
assert_eq!(parent.snapshot()["value"], json!("completed"));
```

Run it: `cargo test -p embed` (or `cargo run -p embed`).

---

## Mode B: JavaScript / WASM

### Install

Install the npm package published with each `vX.Y.Z` release:

```bash
npm install @yestechgroup/wasm
```

…or build the nodejs-target package from source (exact commands, `wasm-bindgen` CLI
version must match the crate in `Cargo.lock`):

```bash
cargo build -p xstate-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target nodejs \
  --out-dir examples/js-embed/pkg --out-name xstate_wasm \
  target/wasm32-unknown-unknown/release/xstate_wasm.wasm
```

The `--target nodejs` output is CommonJS; an ESM demo loads it with
`createRequire(import.meta.url)` (works on Node 20+). See
[`examples/js-embed/README.md`](../examples/js-embed/README.md).

### Init and the validation gate

```javascript
import init, { validateMachine, JsMachine, JsActorSystem, registerGuard, registerAction }
  from '@yestechgroup/wasm';
await init();

// Same checks as validate_machine_json in Rust — run before any spawn.
assert.equal(validateMachine(JSON.stringify(machineIr)), 'ok');
```

### Guards/actions are JS callbacks over real objects

All values crossing the boundary are JSON **strings**; guard/action callbacks receive
**real JS objects** for context and event. Error semantics are pinned
([contracts.md](contracts.md#javascript-api-wasm-restingstatewasm)): a guard that throws
(or returns a non-boolean) **allows** the transition; an action that throws is **skipped**.

```javascript
registerGuard('canSpend', (ctx, event) => ctx.balance >= event.cost);   // real objects
registerAction('recordSpend', (args, event) => { spends.push(event.cost); });
```

Context mutation from JS uses the built-in `assign` action in the machine IR —
`{ name: 'assign', args: [{ $assign_ops: [{ op: 'add', field: 'balance', value: 20 }] }] }`
(op vocabulary in [contracts.md](contracts.md#assign-envelope-ir--interpreter)).

### Send, assert, persist

`JsMachine` mirrors the Rust `Interpreter` surface; snapshots are `{"value", "context"}`
JSON strings and the persisted envelope is `version: 1`:

```javascript
const machine = new JsMachine(JSON.stringify(machineIr));
machine.send(JSON.stringify({ type: 'SPEND', cost: 25 }));   // -> snapshot JSON | null
JSON.parse(machine.snapshot());                              // { value, context }

const persisted = JSON.parse(machine.persistedSnapshot());   // envelope, version 1
const restored = JsMachine.restore(JSON.stringify(machineIr), JSON.stringify(persisted));
```

### Actor system: spawn, spawn-from-state, status

`JsActorSystem` wraps the same native backend; `status` returns the pinned strings
`"running" | "stopped" | "error" | "done"` (`"done"` = child reached a final state):

```javascript
const system = new JsActorSystem();
system.spawnFromState('wallet2', JSON.stringify(machineIr), JSON.stringify(persisted));
system.sendTo('wallet2', JSON.stringify({ type: 'RETRY' }));
JSON.parse(system.snapshot('wallet2'));
system.status('job1');   // "done" after the child's final state
```

Run it: `node examples/js-embed/demo.mjs` (build the pkg first, per the README).

---

## Where to go next

- [contracts.md](contracts.md) — the full API surface, pinned strings, and persistence format.
- [runtime-paths.md](runtime-paths.md) — why all embedding targets the Interpreter path.
- [GUIDE_RUST.md](GUIDE_RUST.md) / [GUIDE_JS.md](GUIDE_JS.md) / [GUIDE_WASM.md](GUIDE_WASM.md) — language guides.
