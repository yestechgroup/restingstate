# restingstate

XState-compatible statechart compiler and runtime for Rust, compiled to WebAssembly.
Parse XState v5 machine definitions (TypeScript), validate them, and run them in the
browser or Node — with guards/actions as JS callbacks, invoked child actors, and
snapshot persistence.

- **Runtime:** the same interpreter that passes the xstate.js compatibility harness
  (compound/parallel/final states, history, eventless + delayed transitions, invoke)
- **One name, two ecosystems:** `npm install restingstate` · `cargo add restingstate`

## Install

```bash
npm install restingstate
```

## Import

```javascript
// Bundlers (browser): named imports resolve to the native-ESM build.
import init, { JsMachine, validateMachine } from 'restingstate';
await init();

// Node: the "node" export condition resolves to CommonJS — use default-import
// (or require). Both expose the identical API surface.
import wasm from 'restingstate';
await wasm.init();
const { JsMachine } = wasm;
```

## Quick start

Always validate machine IR before spawning — `validateMachine` is the contract
LLM-generated machine definitions are checked against:

```javascript
const machineIr = {
  id: 'traffic',
  initial: 'red',
  context: { fields: [] },
  states: [
    { name: 'red',    type: 'atomic', transitions: [{ event: 'NEXT', target: 'green' }] },
    { name: 'green',  type: 'atomic', transitions: [{ event: 'NEXT', target: 'yellow' }] },
    { name: 'yellow', type: 'atomic', transitions: [{ event: 'NEXT', target: 'red' }] },
  ],
};

console.log(wasm.validateMachine(JSON.stringify(machineIr))); // "ok"

const machine = new wasm.JsMachine(JSON.stringify(machineIr));
machine.send(JSON.stringify({ type: 'NEXT' }));
console.log(JSON.parse(machine.snapshot())); // { value: 'green', context: {} }
```

## Guards and actions as JS callbacks

Callbacks receive **real JS objects** (context and event), not strings:

```javascript
wasm.registerGuard('canProceed', (ctx, event) => ctx.count > 0);
wasm.registerAction('track', (args, event) => console.log(event.type));
```

Guards returning `false` (or throwing) block the transition; the machine state is
never left partially updated.

## Actor system

```javascript
const system = new wasm.JsActorSystem();
wasm.registerMachine('worker', JSON.stringify(workerIr));
system.spawn('w1', JSON.stringify(workerIr));
system.sendTo('w1', JSON.stringify({ type: 'PROCESS' }));
console.log(system.snapshot('w1'));   // JSON snapshot
console.log(system.status('w1'));     // "running" | "done" | "stopped" | "error"
```

Machines may `invoke` other registered machines; `done.invoke.{id}` / `error.platform.{id}`
forward to the parent automatically.

## Persistence

```javascript
const persisted = machine.persistedSnapshot();          // versioned JSON string
const restored = wasm.JsMachine.restore(JSON.stringify(machineIr), persisted);
const system2 = new wasm.JsActorSystem();
system2.spawnFromState('w1', JSON.stringify(workerIr), persisted);
```

## Documentation

- Embedding guide (Rust + JS): https://github.com/yestechgroup/restingstate/blob/main/docs/embedding.md
- Dependency contract & API stability: https://github.com/yestechgroup/restingstate/blob/main/docs/contracts.md
- Runtime paths: https://github.com/yestechgroup/restingstate/blob/main/docs/runtime-paths.md

The Rust side ships the same runtime: `cargo add restingstate` (umbrella crate) or
depend on the individual `xstate-*` crates.

## License

MIT OR Apache-2.0
