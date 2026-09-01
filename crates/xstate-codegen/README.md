# xstate-codegen

Rust code generator for [restingstate](https://github.com/yestechgroup/restingstate). Renders an `xstate-ir` machine into typed Rust enums, a context struct, and a `match`-based transition function compatible with `xstate-core`.

See the [root README](../../README.md) for an overview and [docs/runtime-paths.md](../../docs/runtime-paths.md) for the runtime-path decision guide.
