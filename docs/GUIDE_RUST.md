# Rust Developer Guide

## 1. Installation

Add these dependencies to your `Cargo.toml`:

```toml
[dependencies]
xstate-macro = "0.1"
xstate-core = "0.1"
serde = { version = "1", features = ["derive"] }
```

---

## 2. Your First Machine (Proc Macro)

Create `machines/toggle.ts`:

```typescript
import { createMachine } from 'xstate';
export const machine = createMachine({
  id: 'toggle',
  initial: 'inactive',
  context: { count: 0 },
  states: {
    inactive: { on: { TOGGLE: 'active' } },
    active:   { on: { TOGGLE: 'inactive' } }
  }
});
```

Invoke the macro in `main.rs`:

```rust
use xstate_macro::xstate;
use xstate_core::Machine;

xstate!("machines/toggle.ts");

fn main() {
    let machine = MachineImpl;
    let state = machine.initial_state();
    let mut ctx = machine.initial_context();

    assert_eq!(state, State::Inactive);
    assert_eq!(ctx.count, 0);

    let new_state = machine.transition(&state, &Event::Toggle, &mut ctx);
    assert_eq!(new_state, State::Active);
}
```

The file path is relative to your crate root (`CARGO_MANIFEST_DIR`).

---

## 3. Generated Types

`xstate!("machines/toggle.ts")` expands to the following in the calling module:

```rust
#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
pub enum State { Inactive, Active }

#[derive(Debug, Clone, PartialEq, Eq, ::serde::Serialize, ::serde::Deserialize)]
pub enum Event { Toggle }

#[derive(Debug, Clone, ::serde::Serialize, ::serde::Deserialize)]
pub struct Context { pub count: i64 }

pub struct MachineImpl;

impl ::xstate_core::Machine for MachineImpl {
    type State = State;
    type Event = Event;
    type Context = Context;

    fn initial_state(&self) -> Self::State { State::Inactive }
    fn initial_context(&self) -> Self::Context { Context { count: 0 } }

    fn transition(
        &self,
        state: &Self::State,
        event: &Self::Event,
        _context: &mut Self::Context,
    ) -> Self::State {
        match (state, event) {
            (State::Inactive, Event::Toggle) => State::Active,
            (State::Active,   Event::Toggle) => State::Inactive,
            _ => state.clone(),
        }
    }
}
```

Key points:

- **State names** become PascalCase enum variants (`inactive` → `Inactive`, `some_event` → `SomeEvent`).
- **Event names** follow the same conversion (`TOGGLE` → `Toggle`, `PAY_NOW` → `PayNow`).
- **Context fields** map to Rust types: `number` → `i64`, `string` → `String`, `boolean` → `bool`.
- **`Machine::transition`** is a pure match on `(state, event)`. Unknown events return the current state unchanged.
- All generated types implement `Serialize` and `Deserialize`.

---

## 4. Actions

Entry, exit, and transition actions are stored as string identifiers. You supply the behaviour by implementing the `Actions` trait:

```rust
use xstate_core::machine::Actions;

struct MyActions;
impl Actions<Context> for MyActions {
    fn dispatch(&mut self, name: &str, ctx: &mut Context) {
        match name {
            "increment" => ctx.count += 1,
            "reset"     => ctx.count = 0,
            _ => {}
        }
    }
}
```

Define the actions in your `.ts` machine:

```typescript
states: {
  inactive: {
    entry: ['logInactive'],
    on: {
      TOGGLE: { target: 'active', actions: ['increment'] }
    }
  },
  active: {
    exit: ['reset']
  }
}
```

Call `dispatch` yourself after each transition:

```rust
let new_state = machine.transition(&state, &Event::Toggle, &mut ctx);
MyActions.dispatch("increment", &mut ctx);
```

When using the dynamic `Interpreter` (section 7), enter/exit/transition actions are invoked for you automatically.

---

## 5. Guards

Guards are predicate functions keyed by name. Implement the `Guards` trait:

```rust
use xstate_core::machine::Guards;

struct MyGuards;
impl Guards<Context> for MyGuards {
    fn evaluate(&self, name: &str, ctx: &Context) -> bool {
        match name {
            "isPositive" => ctx.count > 0,
            "isAllowed"  => ctx.count < 100,
            _ => true,
        }
    }
}
```

In your TypeScript definition:

```typescript
states: {
  idle: {
    on: { SUBMIT: { target: 'next', guard: 'isPositive' } }
  }
}
```

Check guards before calling `transition`:

```rust
if MyGuards.evaluate("isPositive", &ctx) {
    let new_state = machine.transition(&state, &Event::Submit, &mut ctx);
}
```

---

## 6. Compound & Parallel States

### Compound (Nested) States

```typescript
states: {
  red: {
    initial: 'walk',
    states: {
      walk: { on: { RESET: 'stop' } },
      stop: { type: 'final' }
    }
  },
  green: { on: { TIMER: 'red' } },
  yellow: { on: { TIMER: 'red' } }
}
```

This generates a flat `State` enum with variants `Red`, `Walk`, `Stop`, `Green`, `Yellow` — each child state gets its own variant. All transitions appear in the same match arm. The `Machine::initial_state()` returns the atomic `Red`. For compound tracking use `StateValue` via the dynamic `Interpreter` (section 7).

### Parallel States

```typescript
states: {
  loading: {
    type: 'parallel',
    states: {
      fetching: { on: { DONE: 'loaded' } },
      rendering: { on: { DONE: 'rendered' } }
    }
  }
}
```

The flat enum gets all atomic leaves (`Loading`, `Fetching`, `Rendering`, `Loaded`, `Rendered`). The `DONE` event fires in all parallel regions simultaneously when using the `Interpreter`.

---

## 7. Dynamic Interpretation (No Macro)

When you need to load machines at runtime (from JSON, a database, or dynamic user input), use the `Interpreter` directly. It works with the `xstate_ir::Machine` IR type and string-keyed `StateValue`:

```rust
use xstate_core::{Interpreter, StateValue};
use xstate_ir::Machine;

let json_str = r#"{
    "id": "toggle",
    "initial": "inactive",
    "context": { "fields": [{ "name": "count", "type_name": "number", "default_value": 0 }] },
    "states": [
        { "name": "inactive", "transitions": [{ "event": "TOGGLE", "target": "active" }] },
        { "name": "active", "transitions": [{ "event": "TOGGLE", "target": "inactive" }] }
    ]
}"#;

let machine: Machine = serde_json::from_str(json_str).unwrap();
let interpreter = Interpreter::new(machine);
let state = interpreter.initial_state();
let mut ctx = interpreter.initial_context();

let guard = |_: &str, _: &serde_json::Value| true;
let mut action_fn = |_: &str, _args: &[serde_json::Value]| {};

let new_state = interpreter.send(&state, "TOGGLE", &mut ctx, &guard, &mut action_fn);
```

- `interpreter.initial_state()` returns a `StateValue` (atomic, compound, or parallel).
- `interpreter.initial_context()` returns a `serde_json::Value` object.
- `interpreter.send()` returns `Option<StateValue>` — `None` means no transition matched.
- Guards receive the guard name and the JSON context; return `false` to skip the transition.
- Actions receive the action name and `args` array; execute side effects here.
- Entry/exit actions are fired automatically in the correct XState order.

### StateValue Variants

```rust
StateValue::Atomic("green".into())
StateValue::Compound { state: "processing".into(), child: Box::new(StateValue::Atomic("auth".into())) }
StateValue::Parallel { regions: vec![("fetching".into(), StateValue::Atomic("loaded".into()))] }
```

Serialises to JSON in XState's format: `"green"`, `{"processing":"auth"}`, `{"fetching":"loaded","rendering":"rendered"}`.

---

## 8. Serialization & Snapshots

All generated types derive `serde::Serialize` and `serde::Deserialize`. Use `Machine::start()` for an initial `Snapshot`, or build one manually:

```rust
use xstate_core::{Machine, Snapshot};

let machine = MachineImpl;
let snapshot = machine.start();

let json = serde_json::to_string(&snapshot).unwrap();
// {"state_value":"Inactive","context":{"count":0},"history":[]}

let restored: Snapshot<Context, State> = serde_json::from_str(&json).unwrap();
assert_eq!(restored.state_value, State::Inactive);
assert_eq!(restored.context.count, 0);
```

`Machine::send()` mutates a snapshot — it pushes the previous state to `history`:

```rust
let mut snapshot = machine.start();
machine.send(&mut snapshot, Event::Toggle);
assert_eq!(snapshot.state_value, State::Active);
assert_eq!(snapshot.history.len(), 1);
```

---

## 9. Compile-Time Error Messages

The proc macro provides clear errors at compile time. Fix them before your code ever runs.

| Error | Cause |
|-------|-------|
| `failed to read file 'machines/missing.ts'` | The `.ts` file does not exist at the given path. |
| `failed to parse XState machine: ...` | The TypeScript contains syntax errors or unsupported constructs. Re-run `cargo build` after fixing the `.ts` file. |
| `code generation failed: Unknown type: "object"` | A context field uses a type not supported in IR (`number`, `string`, `boolean` only). Use a simpler type or handle complex data outside context. |
| `initial state '...' not found` | The `initial` field references a state name that isn't defined in `states: { ... }`. |

All of these surface as compiler errors on the `xstate!(...)` invocation line, so you see them inline in your editor or `cargo build` output.

---

## 10. Next Steps

- **Conformance** — see `docs/CONFORMANCE.md` for the full feature support matrix (atomic, compound, parallel, final, guards, actions, and what's planned for 0.2).
- **Examples** — test vectors in `tests/compatibility/machines/` cover every supported feature: `simple/`, `compound/`, `guards/`, `parallel/`, `final/`, `initial/`, `actions/`.
- **Node.js + WASM** — the `xstate-napi` crate lets you run the same machines from JavaScript. See the README for the `JsMachine` API.
- **Contributing** — run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --workspace` before submitting PRs.
