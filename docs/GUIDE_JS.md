# JavaScript Developer Guide

## 1. What restingstate gives you

- **Run XState machines without a JS runtime** — native speed via Rust.
- **Deploy state machines to the browser** via WebAssembly (WASM).
- **Use the same `.ts` machine definitions** you already write with XState v5.
- **Full JSON serialization** for snapshots, persistence, and state transfer.

## 2. Installation

```bash
npm install xstate-napi
```

This is a native `.node` addon, not a pure JS package. You need a Rust toolchain available at install
time (`cargo`). Prebuilt binaries for common platforms are planned.

## 3. Defining Machines

Restingstate uses the same `createMachine({...})` syntax as XState v5. The supported feature set is
documented in [CONFORMANCE.md](./CONFORMANCE.md).

**`toggle.ts`**

```typescript
import { createMachine } from 'xstate';

export const machine = createMachine({
  id: 'toggle',
  initial: 'inactive',
  context: { count: 0 },
  states: {
    inactive: { on: { TOGGLE: 'active' } },
    active: { on: { TOGGLE: 'inactive' } }
  }
});
```

Your `.ts` machine is then compiled to the restingstate JSON IR by the Rust toolchain. For Node.js
usage you work directly with the IR JSON.

## 4. Using JsMachine (Node.js)

`JsMachine` wraps the Rust runtime behind a minimal JavaScript API. All inputs and outputs are JSON
strings — you are responsible for `JSON.stringify` / `JSON.parse`.

```typescript
import { JsMachine } from 'xstate-napi';

const machineJson = JSON.stringify({
  id: 'toggle',
  initial: 'inactive',
  context: { fields: [{ name: 'count', type_name: 'number', default_value: 0 }] },
  states: [
    {
      name: 'inactive',
      node_type: 'Atomic',
      transitions: [{ event: 'TOGGLE', target: 'active' }],
      entry: [],
      exit: [],
      children: [],
      tags: []
    },
    {
      name: 'active',
      node_type: 'Atomic',
      transitions: [{ event: 'TOGGLE', target: 'inactive' }],
      entry: [],
      exit: [],
      children: [],
      tags: []
    }
  ]
});

const machine = new JsMachine(machineJson);

const snap = JSON.parse(machine.snapshot());
console.log(snap); // { value: "inactive", context: { count: 0 } }

const result = machine.send(JSON.stringify({ type: 'TOGGLE' }));
if (result !== null) {
  const parsed = JSON.parse(result);
  console.log(parsed.value);   // "active"
  console.log(parsed.context); // { count: 0 }
}
```

## 5. JsMachine API Reference

| Method | Input | Output | Description |
|--------|-------|--------|-------------|
| `new JsMachine(machineJson)` | JSON string of IR Machine | `JsMachine` instance | Create from pre-parsed IR |
| `JsMachine.fromIr(machineJson, ctxJson)` | Machine IR JSON + initial context JSON | `JsMachine` instance | Create with custom initial context |
| `.snapshot()` | — | JSON string `{"value":...,"context":...}` | Current state and context |
| `.send(eventJson)` | JSON string `{"type":"EVENT"}` | JSON string or `null` | Transition; `null` if no matching transition |

### Nested state values

Compound and parallel states produce structured `value` objects:

```json
{ "value": { "form": "editing" }, "context": { "dirty": true } }
{ "value": { "ui": "visible", "network": "online" }, "context": {} }
```

## 6. Guards

Guards are strings in the machine definition. At runtime, restingstate evaluates a guard by
checking whether the context contains a key matching the guard name with the value `true`.

If the key is absent from the context, the guard **passes** (default: `true`). This lets you
opt in to guarded behavior without pre-populating every guard key.

```typescript
const machineJson = JSON.stringify({
  id: 'auth',
  initial: 'loggedOut',
  context: { fields: [{ name: 'isAdmin', type_name: 'boolean', default_value: false }] },
  states: [
    {
      name: 'loggedOut',
      node_type: 'Atomic',
      transitions: [
        { event: 'LOGIN', guard: { name: 'isAdmin' }, target: 'admin' },
        { event: 'LOGIN', target: 'guest' }
      ],
      entry: [], exit: [], children: [], tags: []
    },
    { name: 'admin', node_type: 'Atomic', transitions: [], entry: [], exit: [], children: [], tags: [] },
    { name: 'guest', node_type: 'Atomic', transitions: [], entry: [], exit: [], children: [], tags: [] }
  ]
});

// Guard passes (isAdmin === true) → transitions to 'admin'
const adminCtx = JSON.stringify({ isAdmin: true });
const m1 = JsMachine.fromIr(machineJson, adminCtx);
const r1 = JSON.parse(m1.send(JSON.stringify({ type: 'LOGIN' }))!);
console.log(r1.value); // "admin"

// Guard fails (isAdmin === false) → falls through to 'guest'
const guestCtx = JSON.stringify({ isAdmin: false });
const m2 = JsMachine.fromIr(machineJson, guestCtx);
const r2 = JSON.parse(m2.send(JSON.stringify({ type: 'LOGIN' }))!);
console.log(r2.value); // "guest"
```

Transitions are evaluated in order. The first transition whose guard passes (or which has no guard)
is taken.

## 7. WASM in the Browser

For browser deployment, compile `xstate-napi` to WebAssembly:

```bash
cargo build -p xstate-napi --target wasm32-unknown-unknown
```

How you load the resulting `.wasm` file depends on your bundler. With `wasm-pack`:

```bash
wasm-pack build crates/xstate-napi --target web
```

Then in your HTML:

```html
<script type="module">
  import init, { JsMachine } from './pkg/xstate_napi.js';

  await init();

  const machineJson = await fetch('/machines/toggle.json').then(r => r.text());
  const machine = new JsMachine(machineJson);
  const result = machine.send(JSON.stringify({ type: 'TOGGLE' }));
  console.log(JSON.parse(result));
</script>
```

## 8. Compatibility

Restingstate v0.1 supports 8 of 9 targeted XState v5 features:

| Feature | Status |
|---------|--------|
| Atomic states | ✓ |
| Compound (nested) states | ✓ |
| Parallel states | ✓ |
| Final states | ✓ |
| Transitions (event → target) | ✓ |
| Guards (cond) | ✓ |
| Context | ✓ |
| Actions (entry/exit/transition) | ✓ |

See [CONFORMANCE.md](./CONFORMANCE.md) for the full conformance matrix, including future targets
(history states, delayed transitions, invoked/spawned actors).

## 9. Next Steps

- **[CONFORMANCE.md](./CONFORMANCE.md)** — full feature conformance detail
- **[examples/js-node/](../examples/js-node/)** — Node.js examples with `JsMachine`
- **[examples/js-wasm/](../examples/js-wasm/)** — browser/WASM integration examples
