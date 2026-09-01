#!/bin/bash
set -e

echo "Building xstate-wasm with wasm-pack..."
wasm-pack build --target web --out-dir pkg --release

echo ""
echo "Build complete. Output in crates/xstate-wasm/pkg/"
echo ""
echo "Usage in HTML:"
echo "  import init, { JsMachine, registerGuard, registerAction } from './pkg/xstate_wasm.js';"
echo "  await init();"
