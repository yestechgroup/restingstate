# Browser WASM Example

This example demonstrates running a restingstate state machine in the browser
via WebAssembly.

## Status

WASM bindings (wasm-bindgen) are planned but not yet implemented.
The HTML file shows the expected API and usage pattern.

## When Available

1. Build the WASM target: `cargo build -p xstate-napi --target wasm32-unknown-unknown`
2. Package with wasm-pack: `wasm-pack build crates/xstate-napi --target web`
3. Serve: `npx serve .`
4. Open `index.html` in a browser

## Current Alternative

For browser usage today, compile your machine to Rust via the proc macro
(`xstate!("machine.ts")`) and build a custom WASM binary with wasm-bindgen.
The xstate-napi crate will provide a ready-made binding in the future.
