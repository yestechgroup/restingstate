# js-embed: executable WASM/JS embedding demo

Node demo for [docs/embedding.md](../../docs/embedding.md) Mode B (issue #33).
`demo.mjs` uses **only** APIs pinned in [docs/contracts.md](../../docs/contracts.md):
`validateMachine` gate, `JsMachine` send/snapshot/persist, `JsMachine.restore`,
`JsActorSystem` (`spawn`, `spawnFromState`, `sendTo`, `snapshot`, `status`),
and JS `registerGuard`/`registerAction` callbacks that receive **real JS objects**
(`context` and `event`), not JSON strings.

## Build the wasm package

`wasm-pack` is not required. The `wasm-bindgen` CLI version **must match** the
`wasm-bindgen` crate in `Cargo.lock` (currently `0.2.127`, checked with
`wasm-bindgen --version`):

```bash
cargo build -p xstate-wasm --target wasm32-unknown-unknown --release
wasm-bindgen --target nodejs \
  --out-dir examples/js-embed/pkg --out-name xstate_wasm \
  target/wasm32-unknown-unknown/release/xstate_wasm.wasm
```

`--target nodejs` emits CommonJS, so the demo loads it with
`createRequire(import.meta.url)` from `demo.mjs` (works on Node 20 and 22).

## Run

```bash
node examples/js-embed/demo.mjs
```

Expected output:

```
js-embed demo passed:
  wallet: {"context":{"balance":30,"spent":0},"value":"locked"}
  persisted version: 1
  actor wallet2: {"context":{"balance":30,"spent":0},"value":"open"}
  job1 status: done
```

All assertions run through `node:assert/strict`; any drift between the docs,
the wasm API surface, and the pinned contract strings fails the demo.

## CI

The demo runs in CI as the last step of the existing `wasm-tests` job (which
already installs the matching `wasm-bindgen` CLI and a Node runtime) — it is
the executable check that keeps this README's build commands honest.

## Machine-IR input

`demo.mjs` feeds hand-written machine-IR JSON (the same `wallet` machine as
[`examples/embed`](../embed), lowered to IR instead of parsed from TypeScript).
The shape is pinned by [`schema/machine-ir.schema.json`](../../schema/machine-ir.schema.json)
and validated at runtime by `validateMachine` before any spawn.
