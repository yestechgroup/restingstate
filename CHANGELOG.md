# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - unreleased

### Added

- JS guard/action callbacks in WASM receive real JS objects (serde-wasm-bindgen
  `json_compatible` serialization); guard/action exceptions surface via
  `console.error` instead of failing silently (#25)
- Registry-aware construction: `MachineLogic::new_with_registries`,
  `ActorSystem::spawn_with_registries` (Tokio) and native-backend registry
  support; `xstate.send_to`/`send_parent`/`spawn` builtins compose with
  registry dispatch (#25)
- Event/context-driven `assign`: `assign(({ context, event }) => ...)` arrow
  updaters are lowered by the parser into a serializable `{"$assign_ops": [...]}`
  IR envelope (set/add/sub, dotted paths, defaults) and applied by the
  interpreter; static assigns stay byte-identical (#25)
- Versioned persisted snapshots `{value, context, history?, entry_times?, version}`
  with restore across Interpreter, Tokio, WASM and NAPI boundaries;
  after-deadlines are preserved across restore (#25)
- Invoke parent/child in the sync native backend: auto-spawn on entering invoke
  states, `done.invoke.{id}` forwarding, `error.platform.{id}` on rejected
  children, stop cascade; WASM `registerMachine`/`childrenOf`/`status: "done"`
  (#25)
- ADR `docs/runtime-paths.md`: the Interpreter is the supported runtime for
  advanced semantics and plugin work; codegen / `xstate!` is scoped to flat
  FSMs (#25)

### Added (release engineering, #32)

- Tag-driven release pipeline (`.github/workflows/release.yml`): wasm-pack npm
  assembly via `scripts/assemble-npm.mjs` (`@yestechgroup/wasm`), ordered
  crates.io publishes (ir → core → parser → backend-native → actor → codegen →
  macro), and a GitHub Release with tarball, wasm sizes and compatibility
  summary
