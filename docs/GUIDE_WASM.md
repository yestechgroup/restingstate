# Browser/WASM Guide

## Quick Start

```bash
# Install wasm32 target
rustup target add wasm32-unknown-unknown

# Build the WASM package
cd crates/xstate-wasm
./build.sh

# Serve the demo
npx serve ../../examples/js-wasm
```

Open `http://localhost:3000` in your browser.

## JavaScript API

### JsMachine (Single Machine)

The simplest API — wraps an XState interpreter for a single machine.

```javascript
import init, { JsMachine } from '@yestechgroup/wasm';
await init();

const machine = new JsMachine(JSON.stringify({
  id: "toggle",
  initial: "inactive",
  context: { fields: [] },
  states: [
    {
      name: "inactive",
      transitions: [{ event: "TOGGLE", target: "active" }],
      children: [], tags: [], entry: [], exit: [], on_done: [],
      output: null, description: null, initial: null, invoke: null,
      type: "atomic"
    },
    {
      name: "active",
      transitions: [{ event: "TOGGLE", target: "inactive" }],
      children: [], tags: [], entry: [], exit: [], on_done: [],
      output: null, description: null, initial: null, invoke: null,
      type: "atomic"
    }
  ]
}));

// Get initial state
console.log(JSON.parse(machine.snapshot()));
// { value: "inactive", context: {} }

// Send events
machine.send(JSON.stringify({ type: "TOGGLE" }));
console.log(JSON.parse(machine.snapshot()).value);
// "active"

// Get state/context individually
console.log(machine.getState());   // JSON string of StateValue
console.log(machine.getContext());  // JSON string of context
```

### JsActorSystem (Multi-Actor)

For running multiple actors with parent-child relationships.

```javascript
import init, { JsActorSystem } from '@yestechgroup/wasm';
await init();

const system = new JsActorSystem();

// Spawn actors from machine IR JSON
system.spawn("worker", JSON.stringify(workerMachineIr));
system.spawn("timer", JSON.stringify(timerMachineIr));

// Send events to specific actors
system.sendTo("worker", JSON.stringify({ type: "PROCESS" }));

// Get snapshots
const snap = JSON.parse(system.snapshot("worker"));
console.log(snap.value, snap.context);

// Stop actors
system.stop("timer");

// Check status
console.log(system.status("worker"));  // "running"
console.log(system.len());              // 1
```

### Guard and Action Registration

Register JavaScript functions as guards and actions for machine transitions.

```javascript
import init, { JsMachine, registerGuard, registerAction } from '@yestechgroup/wasm';
await init();

// Register a guard function
registerGuard("isAuthenticated", (ctx, event) => {
  return ctx.token !== null;
});

// Register an action function
registerAction("logEvent", (args, event) => {
  console.log("Event:", event.type);
});

// Now guards/actions are invoked during transitions
const machine = new JsMachine(JSON.stringify(machineWithGuards));
machine.send(JSON.stringify({ type: "SUBMIT" }));
// The guard "isAuthenticated" will be called with (context, event)
```

### Lifecycle

| Method | Description |
|--------|-------------|
| `new JsMachine(json)` | Create machine from IR JSON |
| `.snapshot()` | Get `{ value, context }` as JSON string |
| `.send(eventJson)` | Send event, return new snapshot or null |
| `.getState()` | Get state value as JSON string |
| `.getContext()` | Get context as JSON string |

| ActorSystem Method | Description |
|-------------------|-------------|
| `new JsActorSystem()` | Create empty actor system |
| `.spawn(id, machineJson)` | Spawn actor from machine IR |
| `.sendTo(id, eventJson)` | Send event to named actor |
| `.snapshot(id)` | Get actor snapshot as JSON string |
| `.stop(id)` | Stop actor |
| `.status(id)` | Get actor status ("running"/"stopped"/"error") |
| `.len()` | Number of actors |
| `.isEmpty()` | Whether system is empty |

## Building from Source

### Prerequisites

- Rust 1.97+ with `wasm32-unknown-unknown` target
- `wasm-pack` (for npm packaging)

```bash
# Install prerequisites
rustup target add wasm32-unknown-unknown
cargo install wasm-pack

# Build for web
cd crates/xstate-wasm
wasm-pack build --target web

# Or use the build script
./build.sh
```

### Build Targets

| Target | Command | Output |
|--------|---------|--------|
| Web (ES module) | `wasm-pack build --target web` | `pkg/xstate_wasm.js` + `.wasm` |
| Node.js | `wasm-pack build --target nodejs` | `pkg/xstate_wasm_bg.wasm` |
| Bundler | `wasm-pack build --target bundler` | For webpack/vite |

## Architecture

```
JavaScript
    │
    ▼
wasm-bindgen (JsMachine, JsActorSystem, register_guard, register_action)
    │
    ▼
xstate-wasm (cdylib)
├── JsMachine → uses Interpreter directly
└── JsActorSystem → uses NativeActorSystem
    │
    ▼
xstate-backend-native (NativeActorSystem — single-threaded, no tokio)
    │
    ▼
xstate-core (Interpreter, ActorLogic, GuardRegistry, ActionRegistry)
    │
    ▼
xstate-ir (Machine, StateNode, Transition IR types)
```

## Limitations

- **Single-threaded**: WASM actors run synchronously on the main thread
- **No async actors**: `CallbackLogic` and `PromiseLogic` are no-ops in WASM mode
- **No delayed transitions**: `MockClock` is used; `after` transitions won't fire in real-time
- **Guard/action callbacks**: Must be registered before sending events
- **No hot reload**: Guards/actions are captured at registration time

## Troubleshooting

**"WASM module not found" in browser**
: Run `cd crates/xstate-wasm && ./build.sh` first.

**"invalid machine JSON" error**
: Ensure your machine IR includes all required fields: `id`, `initial`, `context`, `states`, and each state has `name`, `transitions`, `children`, `tags`, `entry`, `exit`, `on_done`, `output`, `type`.

**Guard/action not called**
: Register them with `registerGuard()` / `registerAction()` before creating the machine.
