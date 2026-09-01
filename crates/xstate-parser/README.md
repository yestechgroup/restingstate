# xstate-parser

OXC-based TypeScript parser for [restingstate](https://github.com/yestechgroup/restingstate). Walks the AST of a `.ts` file, extracts `createMachine(...)` (and `setup().createMachine(...)`) definitions, and emits them as `xstate-ir` machines.

See the [root README](../../README.md) for an overview and [docs/runtime-paths.md](../../docs/runtime-paths.md) for the runtime-path decision guide.
