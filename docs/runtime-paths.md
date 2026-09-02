# ADR: Runtime Paths — Interpreter vs Codegen

**Status:** Accepted
**Date:** 2026-09-01
**Issue:** [#24](https://github.com/yestechgroup/restingstate/issues/24) (part of the EnvHarness gap analysis, #9 / #17)

## Context

The project has two runtime paths that both consume `xstate-ir` `Machine` definitions, but they
implement wildly different subsets of XState semantics:

1. **The Interpreter path** (`crates/xstate-core`) — a tree-walking interpreter
   (`Interpreter`, `crates/xstate-core/src/interpreter.rs:148`) that resolves compound, parallel,
   final, and history states, runs eventless (`always`) and delayed (`after`) transitions, bubbles
   events, orders entry/exit actions, evaluates guards and executes actions via
   `GuardRegistry`/`ActionRegistry` (`crates/xstate-core/src/guards.rs:29`,
   `crates/xstate-core/src/actions.rs:33`, plumbed through `MachineLogic::new_with_registries`,
   `crates/xstate-core/src/actor_logic.rs:76` and `Interpreter::send_with_registry`,
   `crates/xstate-core/src/interpreter.rs:506`),    polls `after` deadlines against the `Clock`
   trait (`crates/xstate-core/src/clock.rs:1`; deadline-gated poll model, never eventless —
   exact semantics in [contracts.md](contracts.md#delayed-transitions-after--exact-semantics)),
   and supports versioned persistence including
   history and after deadlines (`Interpreter::persisted_snapshot` / `restore_persisted`,
   `crates/xstate-core/src/interpreter.rs:283-299`). Invoked and spawned actors are supported on
   this path through the actor backends (Tokio via `xstate-actor`, single-threaded via
   `xstate-backend-native` for WASM). This is the path exercised by the xstate.js compatibility
   harness (25 hand-crafted vectors across 13 categories plus 34 extracted vectors under
   `tests/compatibility/machines/`).

2. **The codegen path** (`crates/xstate-codegen`, exposed via the `xstate!` proc macro in
   `crates/xstate-macro`) — generates a flat `(State, Event) -> State` match
   (`generate_machine_impl`, `crates/xstate-codegen/src/generator.rs:127-200`; the match itself
   at `generator.rs:193-196`). Transition arms carry only the target state
   (`generator.rs:138-146`); guards and transition actions are parsed into the IR but never
   emitted. Delayed transitions are explicitly skipped, both when collecting event variants
   (`generator.rs:67-70`) and when emitting match arms (`generator.rs:133-136`). Entry/exit
   actions, compound/parallel resolution, history, `always`, done transitions, event bubbling,
   multi-target transitions, and invoke have no generated counterpart. The default
   `Machine::send` pushes every previous state onto `Snapshot.history`
   (`crates/xstate-core/src/machine.rs:35`), but the generated `transition` never consults it —
   history is write-only on this path.

Anyone building EnvHarness (or any plugin) against the proc-macro path silently gets a broken
subset of XState: machines parse and compile, but guards never fire, actions never run, and
`after` transitions never happen. Nothing in the docs drew this line.

## Decision

**The Interpreter path is the supported runtime.** All advanced XState semantics, all
conformance work, and all plugin/EnvHarness-style work must target the Interpreter (directly or
through `MachineLogic` and the actor backends).

**The codegen path targets simple flat FSMs and stays that way for now.** It remains useful for
small, deterministic `toggle`-style machines where compile-time enums and zero-overhead
transitions are worth more than semantic fidelity. Its supported surface is exactly:

- atomic states with event → target transitions (including self-transitions via no target)
- a typed `Context` struct generated from `ContextSchema`
- typed `State`/`Event` enums and a total `transition` function

Everything else is out of scope for codegen until further notice.

### Capability matrix

| Capability | Interpreter path | codegen / `#[xstate]` path |
|---|---|---|
| Compound/parallel/final states, history (shallow + deep), `always`, `after` (polled via `Clock`), done transitions, event bubbling, entry/exit ordering, multi-target | ✅ xstate.js-validated (compat harness: 25 hand-crafted + generated vectors under `tests/compatibility/machines/`) | ❌ flat `(State, Event) -> State` match (`crates/xstate-codegen/src/generator.rs:127-200`, match at `:193-196`) |
| Guards/actions at runtime | ✅ `GuardRegistry`/`ActionRegistry` + `MachineLogic::new_with_registries` (`crates/xstate-core/src/actor_logic.rs:76`, `crates/xstate-core/src/interpreter.rs:506`) | ❌ targets only; `delay_ms` explicitly skipped (`crates/xstate-codegen/src/generator.rs:67-70`, `:133-136`) |
| Persistence | ✅ versioned persisted snapshots incl. history + `after` deadlines (`crates/xstate-core/src/interpreter.rs:283-299`) | ❌ |
| Invoke / parent-child actors | ✅ supported on the Interpreter path via the actor backends (Tokio: `xstate-actor`; single-threaded/WASM: `xstate-backend-native`) | ❌ |

### Alternatives considered

**(b) Codegen eventually emitting IR + delegating to the Interpreter** — the generated macro
would embed the machine IR and construct an `Interpreter` instead of emitting a `match`,
preserving proc-macro ergonomics while getting full semantics. **Rejected for now**, because:

- It doubles the maintenance surface: two front doors for the same runtime, with two snapshot
  shapes (typed `State` enum vs `StateValue` tree) to keep in sync.
- The proc-macro's current value proposition — compile-time enums, no runtime IR, flat
  `match` dispatch — is exactly what delegation would remove; users wanting full semantics can
  call the Interpreter directly today (the parser already produces the IR).
- No current consumer needs it: plugin/EnvHarness work targets the Interpreter, and the
  proc-macro's remaining users are flat-FSM machines.

Revisit if proc-macro ergonomics become a stated requirement for plugin authors.

## Consequences

- **Proc-macro users must not expect advanced features.** The `xstate!` docs now carry a
  scoping warning. Machines using guards, actions, `after`, history, compound/parallel states,
  or invoke will compile but behave incorrectly at runtime (guards/actions never run, delayed
  transitions are dropped at codegen time). Feature requests against the codegen path for these
  capabilities will be redirected to the Interpreter path.
- **Plugin work must target the Interpreter.** EnvHarness and any future plugins build on
  `xstate-core` (`Interpreter`, `MachineLogic`, registries, actor backends), not on generated
  code.
- **The conformance profile in `AGENTS.md` is annotated per path** (I = Interpreter,
  C = codegen, B = both) so the split is visible at a glance.
- **Codegen scope changes require an ADR update.** If option (b) is ever revisited, this
  document is the place where the decision gets reversed.
