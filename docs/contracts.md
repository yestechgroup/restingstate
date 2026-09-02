# Dependency Contract

**Audience:** downstream maintainers (EnvHarness and any project consuming `restingstate` as a dependency).
**Scope:** the public API surface, stringly-typed conventions, and persistence format that upgrades must not silently break.
**Issue:** [#28](https://github.com/yestechgroup/restingstate/issues/28) (part of the dependency-readiness epic, #26).

Every signature and behavior below is verified against source; the golden pins live in
`crates/xstate-core/tests/contracts.rs` and `crates/xstate-backend-native/tests/contracts.rs`
(each pinned convention names its test).

## Stable vs beta

Per the epic's crate split:

| Tier | Crates | Promise |
|---|---|---|
| **Public / stable** | `xstate-ir`, `xstate-core`, `xstate-parser`, `xstate-backend-native`, `xstate-actor` | Semver-enforced. Breaking changes require a version bump and a release-notes notice. |
| **Public / beta** | `xstate-macro`, `xstate-codegen` | Consumable, but scoped to simple flat FSMs (see [runtime-paths.md](runtime-paths.md)). Breaking changes may land in minor releases; they will be called out in release notes. |
| **Internal** (`publish = false`) | `xstate-wasm`, `xstate-napi`, `xstate-bench`, `rust-runner`, `examples/*`, `xstate-macro-test` | No stability promise. Consume `xstate-wasm` through the `@restingstate/wasm` npm package instead. |

- **Semver policy:** the workspace ships a single version (`workspace.package`). Until 1.0,
  breaking changes to stable crates bump the minor version; additive changes bump the patch.
  Releases are tag-driven (`vX.Y.Z`) with crates.io and npm artifacts built from the tag (epic #26).
- **MSRV:** **1.97** (`rust-toolchain.toml` pins 1.97.1; `Cargo.toml` declares `rust-version = "1.97"`).
  An MSRV bump is a breaking change.
- **The Interpreter path is the supported runtime.** Codegen/`#[xstate]` is beta and intentionally
  limited; do not build plugins against it ([runtime-paths.md](runtime-paths.md)).

## Rust APIs — `xstate-core`

### Interpreter

`Interpreter` (`crates/xstate-core/src/interpreter.rs:148`) executes an `xstate-ir` `Machine`:
compound/parallel/final/history states, `always`, `after` (against a `Clock`), guards, actions,
`assign`, and event bubbling.

```rust
Interpreter::new(machine: Machine) -> Self                                   // interpreter.rs:173
Interpreter::machine(&self) -> &Machine                                      // :234
Interpreter::initial_state(&self) -> StateValue                              // :238
Interpreter::initial_context(&self) -> serde_json::Value                     // :255
Interpreter::snapshot(&self, state: &StateValue, context: &serde_json::Value)
    -> serde_json::Value                                                     // :272  -> {"value", "context"}
Interpreter::restore(&self, state: &StateValue, context: serde_json::Value)
    -> (StateValue, serde_json::Value)                                       // :263  panics on invalid state
Interpreter::persisted_snapshot(&self, state: &StateValue, context: &serde_json::Value)
    -> serde_json::Value                                                     // :283  see "Persisted snapshot format"
Interpreter::restore_persisted(&self, json: &serde_json::Value)
    -> Result<(StateValue, serde_json::Value), PersistError>                 // :299
```

Two send paths:

```rust
// Closure form (interpreter.rs:401) — used by bindings and embedded hosts:
Interpreter::send(
    &self,
    state: &StateValue,
    event: &serde_json::Value,
    context: &mut serde_json::Value,
    guards: &dyn Fn(&str, &serde_json::Value, &serde_json::Value) -> bool,   // (guard name, context, event)
    actions: &mut dyn FnMut(&str, &[serde_json::Value], &serde_json::Value), // (action name, args, event)
    clock: &dyn Clock,
) -> Option<StateValue>   // Some(new state) if the configuration changed, None otherwise

// Registry form (interpreter.rs:506) — same semantics, GuardRegistry/ActionRegistry instead of closures:
Interpreter::send_with_registry(
    &self, state: &StateValue, event: &serde_json::Value, context: &mut serde_json::Value,
    guards: &GuardRegistry, actions: &ActionRegistry, clock: &dyn Clock,
) -> Option<StateValue>
```

`Send` returns the new `StateValue`; `context` is mutated in place (including by `assign`).
The event object must carry a `"type"` string. `Clock` is `fn now_ms(&self) -> u64`
(`crates/xstate-core/src/clock.rs:1`) with `MockClock`, `InstantClock` (native), and `WasmClock` implementations.

### Delayed transitions (`after`) — exact semantics

Pinned by `crates/xstate-core/tests/after_parity.rs`; fixed in
[#34](https://github.com/yestechgroup/restingstate/issues/34) (before the fix, delayed
transitions could fire eventlessly on the next microstep, ignoring the delay).

- **Deadline math:** `deadline = entered_at(source state node) + delay_ms`, both in the units of
  the injected `Clock`. `entry_times` is recorded on entry (initial entry counts as `t=0` on the
  `Clock`) and is part of the persisted snapshot (see [#22](https://github.com/yestechgroup/restingstate/issues/22)).
- **Poll model — no real-time scheduling:** `after` fires on the **first `send` whose `Clock`
  reading is at or past the deadline**. Nothing fires between sends; hosts that need wall-clock
  `after` must drive sends (or a timer that sends a tick event).
- **Never eventless, never direct:** a transition with `delay_ms` never matches in
  `send_along_path` — not via the eventless (`always`/`resolve_always`) path and not when its
  (named) event is sent directly. `resolve_after` is the sole firer and re-checks guards at fire
  time; a false guard leaves the machine unchanged and the transition is retried on later sends.
- **Early events do not fire it:** an event handled while `now < deadline` leaves the machine
  unchanged with respect to the pending `after`.
- **Exit cancels permanently:** leaving the source state before the deadline discards its
  pending `after`; it never fires later from another state.
- **Re-entry resets the deadline:** re-entering the source state (including an external
  self-target transition, e.g. `on: { STAY: 'green' }` on `green`) restarts the countdown.
  An `after` targeting its own state re-arms itself the same way.
- **Deepest-first when multiple are due:** if several `after` deadlines are due in the same
  resolve, the active path is walked leaf-first, so the deepest state's transition fires first;
  ancestors' pending `after`s are cancelled if the deeper fire exits them.
- **Observability:** the fired transition is returned by the `send` that observed the deadline;
  `send` returns `Some(new_state)` only when the configuration changed.

Compat note: `tests/compatibility/machines/after/*` vectors now compare against xstate.js under
this semantics (see [#34](https://github.com/yestechgroup/restingstate/issues/34)).

`PersistError` (`interpreter.rs:156`): `MissingValue`, `UnsupportedVersion(u64)`,
`InvalidStateValue(String)`, `InvalidHistory(String)`, `InvalidEntryTimes(String)`,
`HistoryTargetInvalid(String)`. Restore validates state values against the machine before accepting them.

### GuardRegistry / ActionRegistry

```rust
// crates/xstate-core/src/guards.rs
GuardImpl::new(name: impl Into<String>,
    f: impl Fn(&serde_json::Value /*context*/, &serde_json::Value /*event*/) -> bool + Send + Sync + 'static)
GuardRegistry::new() / register(guard) / has(name) / evaluate(name, context, event) -> bool
guards::and(Vec<GuardImpl>) | guards::or(Vec<GuardImpl>) | guards::not(GuardImpl)   // :68, :77, :86

// crates/xstate-core/src/actions.rs
ActionImpl::new(name: impl Into<String>,
    f: impl Fn(&[serde_json::Value] /*args*/, &serde_json::Value /*event*/, &mut serde_json::Value /*context*/)
        + Send + Sync + 'static)
ActionRegistry::new() / register(action) / has(name)
    / dispatch(name, args: &[Value], event: &Value, context: &mut Value)              // :48
actions::sequence(Vec<ActionImpl>) | actions::parallel(Vec<ActionImpl>)              // :72, :83
actions::assign_action(updater: impl Fn(&Value /*context*/, &Value /*event*/) -> Value)  // :94
```

**Contract:** a **missing guard allows** the transition (`evaluate` returns `true` for unknown
names, `guards.rs:44`) and a **missing action is a no-op** (`dispatch` ignores unknown names,
`actions.rs:48`). Built-in action names (`xstate.*`, see below) always take precedence over
registry entries with the same name.

### ActorLogic and MachineLogic

```rust
// crates/xstate-core/src/actor_logic.rs:36
pub trait ActorLogic: Send + Sync {
    fn name(&self) -> &str;
    fn initial_snapshot(&self) -> serde_json::Value;
    fn receive(&mut self, event: serde_json::Value, snapshot: &mut serde_json::Value) -> Vec<serde_json::Value>;
    fn stop(&mut self) -> Option<serde_json::Value> { None }
}
```

Implementations: `MachineLogic` (wraps an `Interpreter`), `ReducerLogic`, `CallbackLogic`,
`PromiseLogic` (all `actor_logic.rs`). `MachineLogic` snapshots are
`{"value": <StateValue>, "context": <Value>}`; `MachineLogic::name()` is the machine's `id`.

```rust
MachineLogic::new(interpreter: Interpreter, clock: impl Clock + 'static) -> Self                 // :67
MachineLogic::new_with_registries(interpreter, clock, guards: GuardRegistry, actions: ActionRegistry)  // :76
MachineLogic::with_initial_snapshot(self, snapshot: serde_json::Value) -> Self                   // :93
MachineLogic::persisted_snapshot(&self, snapshot: &serde_json::Value) -> serde_json::Value       // :98
    // running {"value","context"}  ->  full persisted envelope (below)
MachineLogic::restore_persisted(&self, json: &serde_json::Value)
    -> Result<serde_json::Value, PersistError>                                                   // :109
    // persisted envelope  ->  running {"value","context"}
```

`PromiseLogic` snapshots use `"status"`: `"pending"` / `"resolved"` (+`"value"`) / `"rejected"`
(+`"error"`). On the native backend a rejected promise marks the actor `Error` and emits
`error.platform.{id}` to its parent.

### Backend traits

`crates/xstate-core/src/backend.rs` — implemented by both backends:

```rust
enum ActorStatus { Running, Stopped, Error, Done }                       // :6
trait ActorHandle { send(event); stop(); snapshot() -> Value; status() -> ActorStatus }  // :14
trait ActorSystem {                                                      // :26
    type Handle: ActorHandle;
    spawn(id: &str, logic: Box<dyn ActorLogic>) -> Handle;
    send_to(id: &str, event: Value); stop(id: &str);
    snapshot(id: &str) -> Option<Value>; status(id: &str) -> Option<ActorStatus>;
    len() -> usize; is_empty() -> bool;
}
```

## Actor backends

### `NativeActorSystem` (xstate-backend-native, single-threaded / WASM)

`crates/xstate-backend-native/src/lib.rs:318`. Synchronous: `send`/`dispatch` drain the effect
queue (including child spawns and done/error forwarding) before returning.

```rust
NativeActorSystem::new() -> Self
register_machine(&self, name: &str, machine: Machine)                    // :334  enables xstate.spawn + invoke
set_registry_factory(&self, factory: impl Fn() -> (GuardRegistry, ActionRegistry) + Send + Sync + 'static)  // :342
spawn_machine(&self, id: &str, machine: Machine) -> NativeActorHandle                    // :349
spawn_machine_with_parent(&self, id: &str, machine: Machine, parent: &str) -> NativeActorHandle  // :353
spawn_machine_from_persisted(&self, id: &str, machine: Machine, persisted: &Value)
    -> Result<NativeActorHandle, PersistError>                                           // :362
spawn_logic_with_parent(&self, id: &str, logic: Box<dyn ActorLogic>, parent: &str) -> NativeActorHandle // :390
children_of(&self, id: &str) -> Vec<String>                              // :458
dispatch(&self, id: &str, event: Value) / stop_actor(&self, id)          // :468, :472
get_snapshot(&self, id) -> Option<Value> / get_status(&self, id) -> Option<ActorStatus>   // :476, :485
```

It also implements the `ActorSystem` trait above (`lib.rs:531`), so `send_to`/`status`/`snapshot`
work through the trait. Stopping an actor cascades to its descendants.

Invoke behavior: entering a state with `invoke` auto-spawns the child as `invoke.id` or
`{parent_actor_id}.{src}` (`lib.rs:238`). IR `onDone`/`onError` transitions are rewritten at spawn
time to the actor-scoped event names (`rewrite_invoke`, `lib.rs:90`).

### `ActorSystem` (xstate-actor, Tokio)

`crates/xstate-actor/src/system.rs:47`. Async tasks per actor; `xstate-actor` defines its own
`ActorStatus`/`ActorHandle` (`handle.rs:5`, `handle.rs:18`) with the same variants/shape.

```rust
ActorSystem::new() -> Self
register_machine(&self, name: &str, machine: Machine)                    // system.rs:135
spawn(&self, interpreter: Interpreter, clock: impl Clock + 'static, id: String) -> String        // :144
spawn_with_registries(&self, interpreter, clock, id, guards: GuardRegistry, actions: ActionRegistry) -> String  // :164
spawn_with_persisted(&self, interpreter, clock, id: String, persisted: &Value)
    -> Result<String, PersistError>                                      // :310
spawn_with_state(...) / spawn_with_parent(...) / spawn_logic(...)        // :281, :239, :190
send_to(&self, id: &str, event: Value)                                   // :336
stop(&self, id: &str) / snapshot(&self, id) -> Option<Value> / status(&self, id) -> Option<ActorStatus>  // :343-353
broadcast(event) / stop_all() / gc() / len() / is_empty() / subscribe(actor_id, callback)  // :358-390
```

Backend difference to know: **done forwarding works on both backends; `error.platform`
forwarding for rejected promises is implemented on the native backend** (the Tokio task loop
forwards `done.invoke` on final states, `actor.rs:186`, but does not emit error events).
If you need promise-rejection plumbing today, use `xstate-backend-native` or handle it in the host.

## Parser (`xstate-parser`)

```rust
xstate_parser::parse(source: &str) -> Result<Machine, ParseError>        // parser.rs:15
xstate_parser::parse_all(source: &str) -> Result<Vec<Machine>, ParseError>  // :28
```

- Both reject sources with TypeScript syntax errors (`ParseError::OxcParse`).
- `parse()` extracts the **first** `createMachine` call and fails with
  `InvalidMachine("No createMachine call found")` if there is none.
- `parse_all()` collects **every** `createMachine` in the file, including inside function bodies,
  object/array literals, and `if`/block statements.
- `assign({ field: ({ context, event }) => ... })` updaters are lowered to the `$assign_ops`
  envelope (below); unsupported updater shapes are hard parse errors, never silently dropped.

## Stringly-typed conventions (pinned)

These strings cross crate and language boundaries. **Renaming any of them is a breaking
change**; each is locked by a golden test.

### Event names

| Event | Emitted when | Payload | Delivered by |
|---|---|---|---|
| `done.invoke.{child_id}` | a child machine actor reaches a final state | `{"output": <child context>}` | native + Tokio backends |
| `error.platform.{child_id}` | an invoked/spawned child actor fails (rejected promise on the native backend) | `{"error": <error value>}` | native backend |

- `{child_id}` is the child's **actor id**: the explicit `invoke.id`, else `{parent_actor_id}.{src}`
  for invoke auto-spawn (`backend-native lib.rs:238`, `xstate-actor actor.rs:222`), else the id you
  passed to `spawn_machine_with_parent`/`spawn_with_parent`.
- The parser translates `invoke.onDone`/`onError` configs to IR transitions named
  `done.invoke.{id-or-src}` / `error.platform.{id-or-src}` (`parser.rs:1211`, `:1237`); the native
  backend rewrites them to the actor-scoped names at spawn time. On the Tokio backend the machine
  must carry the actor-scoped names itself (or set explicit invoke ids).
- Pins: `done_invoke_event_type_and_output_payload_are_pinned`,
  `invoke_without_explicit_id_delivers_done_invoke_parent_dot_src`,
  `error_platform_event_type_and_error_payload_are_pinned`
  (`crates/xstate-backend-native/tests/contracts.rs`).

### Built-in action names

| Action | Args (positional) | Effect |
|---|---|---|
| `xstate.send_to` | `[target_id: string, event: object]` (event defaults to `null` if omitted) | delivers `event` to actor `target_id` |
| `xstate.send_parent` | `[event: object]` | delivers `event` to the actor's parent (no-op without a parent) |
| `xstate.spawn` | `[machine_name: string, child_id?: string]` | spawns a child from the machine registered under `machine_name`; default id is `{spawning_actor_id}.child` |

Reserved on both backends (`backend-native lib.rs:132-184`, `xstate-actor actor.rs:129-175`);
registry entries with these names are ignored. Pins:
`xstate_send_to_arg_positions_are_pinned`, `xstate_send_parent_arg_position_is_pinned`,
`xstate_spawn_arg_positions_and_default_child_id_are_pinned`.

### `assign` envelope (IR → interpreter)

The parser emits dynamic `assign` as an `Action { name: "assign" }` whose **first** arg is exactly:

```json
{ "$assign_ops": [ <op>, ... ] }
```

(`parser.rs:1417`; emission pinned in `crates/xstate-parser/tests/parser_tests.rs`. The envelope
key is `ASSIGN_OPS_KEY`, `interpreter.rs:10`.)

Op vocabulary (applied in order by `apply_assign_ops`, `interpreter.rs:44`):

| Op | Shape | Meaning |
|---|---|---|
| set (literal) | `{"op": "set", "field": F, "value": V}` | set `F` to `V` |
| set (copy) | `{"op": "set", "field": F, "from": "event.a.b" \| "context.x", "default": D?}` | copy from a dotted event/context path; `D` (literal) when the path is missing |
| add / sub | `{"op": "add" \| "sub", "field": F, "value": N, "default": D?}` | numeric delta on `F`; falls back to `D` when `F` is unset |

`F` may be a dotted path (missing intermediate objects are created); integral results are
normalized to JSON integers. An `assign` action **without** the envelope falls back to pairwise
literal assignment: `args = [key1, value1, key2, value2, ...]`. Pins:
`assign_ops_envelope_in_first_arg_is_applied`, `assign_pairwise_literal_fallback_is_pinned`
(`crates/xstate-core/tests/contracts.rs`).

## Persisted snapshot format

`Interpreter::persisted_snapshot` emits exactly five top-level keys
(`interpreter.rs:283`; pin: `persisted_snapshot_envelope_is_pinned`):

```json
{
  "value":       <StateValue>,
  "context":     <Value>,
  "history":     { "<state name>": <StateValue> },
  "entry_times": { "<state name>": <ms at entry> },
  "version":     1
}
```

- `version` equals `PERSISTED_SNAPSHOT_VERSION` (`interpreter.rs:154`, currently `1`).
  `restore_persisted` accepts a missing `version` but **rejects any value other than `1`**
  (`PersistError::UnsupportedVersion`).
- **Evolution rule: additive-only.** New optional keys may be added with a version bump;
  existing keys keep their meaning and required-ness. Consumers must ignore unknown keys.
- **Back-compat guarantee:** a legacy `{"value", "context"}` snapshot (no version/history/
  entry_times) still restores; a missing `context` restores as `null`.
- `Interpreter::snapshot` (the non-persisted form) stays exactly `{"value", "context"}` —
  the shape actors use at rest (`pin: plain_snapshot_shape_is_pinned`).
- `MachineLogic::persisted_snapshot` / `restore_persisted` convert between the running
  `{"value","context"}` shape and this envelope (pin:
  `machine_logic_persisted_round_trip_shape_is_pinned`).

## JavaScript API (WASM, `@restingstate/wasm`)

Built from `crates/xstate-wasm` (wasm-bindgen). All values cross the boundary as **JSON strings**;
guard/action callbacks receive **JS objects** (not strings).

```javascript
import init, { JsMachine, JsActorSystem, registerGuard, registerAction, clearRegistry }
  from '@restingstate/wasm';
await init();

// Single machine (crates/xstate-wasm/src/lib.rs:117)
const machine = new JsMachine(machineIrJson);        // xstate-ir Machine JSON
machine.send(eventJson);        // -> snapshot JSON string | null (no transition)
machine.snapshot();             // -> {"value", "context"} JSON string
machine.persistedSnapshot();    // -> full persisted envelope (above), version 1
JsMachine.restore(machineIrJson, persistedJson);  // -> JsMachine (restore_persisted semantics)
machine.getContext(); machine.getState();

// Actor system (lib.rs:256)
const system = new JsActorSystem();
system.spawn(id, machineIrJson);                     // NativeActorSystem::spawn_machine
system.registerMachine(name, machineIrJson);         // enables xstate.spawn / invoke
system.spawnFromState(id, machineIrJson, persistedJson);   // spawn_machine_from_persisted
system.spawnReducer(id, initialJson);
system.sendTo(id, eventJson);
system.snapshot(id);            // JSON string | null
system.status(id);              // "running" | "stopped" | "error" | "done" | null
system.childrenOf(id);          // JSON array of child ids
system.stop(id); system.len(); system.isEmpty();

// JS registries (applied to every spawned machine actor)
registerGuard("name", (contextObj, eventObj) => boolean);
registerAction("name", (argsArray, eventObj) => void);
clearRegistry();
```

Callback error semantics: a **guard that throws is logged and treated as allowing** the
transition (`lib.rs:56`); an **action that throws is logged and skipped** (`lib.rs:79`). A guard
returning a non-boolean is treated as `true`. Status strings are pinned by
`crates/xstate-wasm/tests/actors.rs` (including `"done"`); the wasm tests run in CI (epic #31).

## Machine-IR schema & validation

The machine-IR contract artifact is the committed JSON Schema
[`schema/machine-ir.schema.json`](../schema/machine-ir.schema.json) (draft 2020-12,
generated with `schemars` from the `xstate-ir` types).

- **Generation** is opt-in: `xstate-ir`'s `schema` feature (default OFF) derives
  `JsonSchema` on every public IR type and exposes `machine_ir_schema()` /
  `machine_ir_schema_json()`. A golden test regenerates the file and fails on drift —
  regenerate with `XSTATE_REGEN_SCHEMA=1 cargo test -p xstate-ir --features schema`
  and commit the result together with the IR change.
- **Rust validation:** `xstate_core::validate_machine_json(&str) -> Result<(), ValidateError>`
  (`crates/xstate-core/src/validation.rs`) parses the JSON into `xstate_ir::Machine`
  (`ValidateError::Parse`) and runs the structural checks shared with
  `Interpreter::validate_machine` (`ValidateError::Invalid`): initial resolvability,
  compound initial children, and transition-target existence (dotted targets are
  root-anchored, bare names resolve anywhere in the tree — mirroring `build_target`).
- **WASM validation:** `validateMachine(machineIrJson)` from `@restingstate/wasm`
  (`crates/xstate-wasm/src/lib.rs`) returns `"ok"` or a rejecting error message using
  the same checks.
- **Sweep:** every hand-crafted compatibility vector's `machine_ir` is asserted to
  parse and validate (`crates/xstate-core/tests/compat_machine_validation.rs`);
  schema drift is caught by the golden test above.

## Where the pins live

| Contract | Test |
|---|---|
| `done.invoke.{id}` type + `output` payload; `{parent}.{src}` default id | `crates/xstate-backend-native/tests/contracts.rs` |
| `error.platform.{id}` type + `error` payload | `crates/xstate-backend-native/tests/contracts.rs` |
| `xstate.send_to` / `xstate.send_parent` / `xstate.spawn` arg positions | `crates/xstate-backend-native/tests/contracts.rs` |
| `$assign_ops` envelope + op vocabulary; pairwise fallback | `crates/xstate-core/tests/contracts.rs`, `crates/xstate-parser/tests/parser_tests.rs` |
| Persisted snapshot key set, `version: 1`, legacy restore, rejection rules | `crates/xstate-core/tests/contracts.rs`, `crates/xstate-core/tests/persistence_tests.rs` |
| Guard/action closure shapes; missing-guard→allow / missing-action→noop | `crates/xstate-core/tests/contracts.rs` |
| Machine-IR JSON Schema golden; validate_machine / wasm `validateMachine` | `crates/xstate-ir/tests/schema_golden.rs`, `crates/xstate-core/tests/validation_tests.rs`, `crates/xstate-wasm/tests/validate.rs` |
| wasm status strings / JS API | `crates/xstate-wasm/tests/` |

A change to any pinned string, payload key, or snapshot key set must fail these tests before it
can ship — that failure is the signal to bump the version and document the migration.
