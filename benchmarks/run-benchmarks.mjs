import {
  createMachine,
  createActor,
  fromCallback,
  assign,
} from 'xstate';
import { performance } from 'perf_hooks';
import { createRequire } from 'module';
const require = createRequire(import.meta.url);

import { readFileSync, writeFileSync, existsSync, mkdirSync, readdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import os from 'os';

const __dirname = dirname(fileURLToPath(import.meta.url));
const RESULTS_DIR = join(__dirname, 'results');
const WORKLOADS_DIR = join(__dirname, 'workloads');
const PROJECT_ROOT = join(__dirname, '..');

const WARMUP = 1000;
const MEASURED = 100_000;
const TRIALS = 5;

function loadWorkload(name) {
  const path = join(WORKLOADS_DIR, `${name}.json`);
  if (!existsSync(path)) {
    throw new Error(`Workload not found: ${name}`);
  }
  return JSON.parse(readFileSync(path, 'utf-8'));
}

function listWorkloads() {
  const files = readdirSync(WORKLOADS_DIR);
  return files
    .filter((f) => f.endsWith('.json'))
    .map((f) => f.replace('.json', ''))
    .sort();
}

function aggregate(results) {
  const startups = results.map((r) => r.startup_ms);
  const totals = results.map((r) => r.total_ms);
  const throughputs = results.map((r) => r.throughput_eps);
  const snapshots = results.map((r) => r.snapshot_bytes);

  const mean = (arr) => arr.reduce((a, b) => a + b, 0) / arr.length;
  const min = (arr) => Math.min(...arr);
  const max = (arr) => Math.max(...arr);

  const tMean = mean(throughputs);
  return {
    startup_ms: {
      mean: mean(startups),
      min: min(startups),
      max: max(startups),
    },
    total_ms: { mean: mean(totals), min: min(totals), max: max(totals) },
    throughput_eps: {
      mean: tMean,
      min: min(throughputs),
      max: max(throughputs),
    },
    snapshot_bytes: { mean: mean(snapshots) },
    trials: results.length,
  };
}

function collectGuards(config) {
  const names = new Set();
  function walk(node) {
    if (node.on) {
      for (const trans of Object.values(node.on)) {
        const items = Array.isArray(trans) ? trans : [trans];
        for (const t of items) {
          if (typeof t === 'object' && t !== null && typeof t.guard === 'string') {
            names.add(t.guard);
          }
        }
      }
    }
    if (node.always) {
      const items = Array.isArray(node.always) ? node.always : [node.always];
      for (const t of items) {
        if (typeof t === 'object' && t !== null && typeof t.guard === 'string') {
          names.add(t.guard);
        }
      }
    }
    if (node.states) {
      for (const child of Object.values(node.states)) {
        walk(child);
      }
    }
  }
  walk(config);
  return names;
}

function collectInvokes(config) {
  const names = new Set();
  function walk(node) {
    if (node.invoke) {
      const sources = Array.isArray(node.invoke)
        ? node.invoke
        : [node.invoke];
      for (const inv of sources) {
        if (typeof inv.src === 'string') {
          names.add(inv.src);
        }
      }
    }
    if (node.states) {
      for (const child of Object.values(node.states)) {
        walk(child);
      }
    }
  }
  walk(config);
  return names;
}

function buildGuardImpls(guardNames) {
  const guards = {};
  for (const name of guardNames) {
    guards[name] = ({ context, event }) => {
      if (context !== null && typeof context === 'object' && name in context) {
        return Boolean(context[name]);
      }
      if (event && typeof event === 'object' && name in event) {
        return Boolean(event[name]);
      }
      if (name === 'isPositive' && event && typeof event === 'object' && 'value' in event) {
        return event.value > 0;
      }
      if (name === 'seconds_eq_0') {
        return context !== null
          && typeof context === 'object'
          && context.seconds === 0;
      }
      if (name === 'seconds_gt_0') {
        return context !== null
          && typeof context === 'object'
          && context.seconds > 0;
      }
      return true;
    };
  }
  return guards;
}

function buildActorImpls(invokeNames) {
  const actors = {};
  for (const name of invokeNames) {
    if (name === 'ticks') {
      actors[name] = fromCallback(({ sendBack }) => {
        let count = 0;
        const iv = setInterval(() => {
          if (++count > 50) {
            clearInterval(iv);
            return;
          }
          sendBack({ type: 'TICK' });
        }, 10);
        return () => clearInterval(iv);
      });
    } else {
      actors[name] = fromCallback(() => {
        return () => {};
      });
    }
  }
  return actors;
}

function benchXstateJs(machineConfig, events) {
  const guardNames = collectGuards(machineConfig);
  const invokeNames = collectInvokes(machineConfig);
  const guards = buildGuardImpls(guardNames);
  const actors = buildActorImpls(invokeNames);

  const results = [];
  for (let trial = 0; trial < TRIALS; trial++) {
    const startStart = performance.now();
    const machine = createMachine(machineConfig, { guards, actors });
    const actor = createActor(machine).start();
    const startEnd = performance.now();

    for (let i = 0; i < WARMUP; i++) {
      try {
        actor.send(events[i % events.length]);
      } catch {}
    }

    const benchStart = performance.now();
    let eventIdx = 0;
    for (let i = 0; i < MEASURED; i++) {
      try {
        actor.send(events[eventIdx % events.length]);
      } catch {}
      eventIdx++;
    }
    const benchEnd = performance.now();

    const snapshot = actor.getSnapshot();
    results.push({
      startup_ms: startEnd - startStart,
      total_ms: benchEnd - benchStart,
      throughput_eps: Math.round(
        MEASURED / ((benchEnd - benchStart) / 1000)
      ),
      snapshot_bytes: JSON.stringify(snapshot).length,
    });
  }
  return results;
}

function loadWasmModule() {
  const wasmPkg = join(__dirname, 'wasm-pkg', 'xstate_wasm.cjs');
  if (!existsSync(wasmPkg)) {
    console.error(
      'WASM package not found. Build with:',
      'cargo build -p xstate-wasm --target wasm32-unknown-unknown --release &&',
      'wasm-bindgen --target nodejs --out-dir benchmarks/wasm-pkg target/wasm32-unknown-unknown/release/xstate_wasm.wasm'
    );
    return null;
  }
  const mod = require(wasmPkg);
  mod.init();
  return mod;
}

const WASM_MEASURED = 100_000;

function benchWasmDirect(wasmMod, machineIr, events) {
  const initialCtx = {};
  if (machineIr.context && machineIr.context.fields) {
    for (const f of machineIr.context.fields) {
      initialCtx[f.name] = f.default_value;
    }
  }

  const machineJson = JSON.stringify(machineIr);
  const results = [];

  for (let trial = 0; trial < TRIALS; trial++) {
    const benchEvents = [];
    for (let i = 0; i < WASM_MEASURED; i++) {
      benchEvents.push(events[i % events.length]);
    }

    const startStart = performance.now();
    const m = new wasmMod.JsMachine(machineJson);
    const startEnd = performance.now();

    const benchStart = performance.now();
    for (let i = 0; i < WASM_MEASURED; i++) {
      m.send(JSON.stringify(benchEvents[i]));
    }
    const benchEnd = performance.now();

    const snap = JSON.parse(m.snapshot());
    results.push({
      startup_ms: startEnd - startStart,
      total_ms: benchEnd - benchStart,
      throughput_eps: Math.round(
        WASM_MEASURED / ((benchEnd - benchStart) / 1000)
      ),
      snapshot_bytes: JSON.stringify(snap).length,
    });
  }

  return results;
}

function benchmarkWorkload(name, wasmMod) {
  console.log(`\nBenchmarking: ${name}`);
  const workload = loadWorkload(name);
  const events = workload.events;

  console.log(`  xstate.js: ${TRIALS} trials...`);
  const xstateResults = benchXstateJs(workload.machine_config, events);
  const xstateAgg = aggregate(xstateResults);
  console.log(
    `    ${xstateAgg.throughput_eps.mean.toLocaleString()} eps `
    + `(startup ${xstateAgg.startup_ms.mean.toFixed(3)} ms)`
  );

  console.log(`  xstate-wasm: ${TRIALS} trials...`);
  const wasmResults = benchWasmDirect(wasmMod, workload.machine_ir, events);
  const wasmAgg = wasmResults.length > 0 ? aggregate(wasmResults) : null;

  if (wasmAgg) {
    console.log(
      `    ${wasmAgg.throughput_eps.mean.toLocaleString()} eps`
    );
  } else {
    console.log(`    (skipped — no results)`);
  }

  const speedup =
    wasmAgg && wasmAgg.throughput_eps.mean > 0
      ? wasmAgg.throughput_eps.mean / xstateAgg.throughput_eps.mean
      : null;

  return {
    workload: name,
    warmup_events: WARMUP,
    measured_events: MEASURED,
    trials: TRIALS,
    xstate_js: xstateAgg,
    xstate_wasm: wasmAgg,
    speedup,
  };
}

function main() {
  if (!existsSync(RESULTS_DIR)) {
    mkdirSync(RESULTS_DIR, { recursive: true });
  }

  console.log('Loading WASM module...');
  const wasmMod = loadWasmModule();
  if (!wasmMod) {
    process.exit(1);
  }
  console.log('WASM module loaded.\n');

  const workloads = listWorkloads();
  console.log(`Workloads: ${workloads.join(', ')}`);
  console.log(
    `Config: warmup=${WARMUP.toLocaleString()}, `
    + `measured=${MEASURED.toLocaleString()}, trials=${TRIALS}`
  );

  const allResults = [];
  for (const name of workloads) {
    const result = benchmarkWorkload(name, wasmMod);
    allResults.push(result);
  }

  const systemInfo = {
    os: `${os.type()} ${os.release()}`,
    cpu: os.cpus()[0]?.model || 'unknown',
    node: process.version,
    wasm: 'direct (wasm-bindgen, in-process)',
    timestamp: new Date().toISOString(),
    warmup_events: WARMUP,
    measured_events: MEASURED,
    trials: TRIALS,
  };

  const report = {
    system: systemInfo,
    results: allResults,
  };

  const outPath = join(RESULTS_DIR, 'latest.json');
  writeFileSync(outPath, JSON.stringify(report, null, 2));
  console.log(`\nResults written to ${outPath}`);

  const md = generateMarkdown(report);
  const mdPath = join(RESULTS_DIR, 'report.md');
  writeFileSync(mdPath, md);
  console.log(`Report written to ${mdPath}\n`);
  console.log(md);
}

function generateMarkdown(report) {
  const { system, results } = report;

  let md = '## Performance Benchmarks\n\n';
  md += `System: ${system.os}, CPU: ${system.cpu}\n`;
  md += `Node: ${system.node}, Rust: ${system.rust}\n`;
  md += `Warmup: ${system.warmup_events.toLocaleString()} events, `;
  md += `Measured: ${system.measured_events.toLocaleString()} events, `;
  md += `Trials: ${system.trials}\n`;
  md += `Timestamp: ${system.timestamp}\n\n`;

  md +=
    '| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup | Startup (js / wasm) |\n';
  md +=
    '|----------|----------------|-------------------|---------|--------------------|\n';

  for (const r of results) {
    const jsEps = r.xstate_js.throughput_eps.mean.toLocaleString();
    const wasmEps = r.xstate_wasm
      ? r.xstate_wasm.throughput_eps.mean.toLocaleString()
      : '—';
    const speedup = r.speedup ? `${r.speedup.toFixed(1)}x` : '—';
    const jsStartup = `${r.xstate_js.startup_ms.mean.toFixed(3)} ms`;
    const wasmStartup = r.xstate_wasm
      ? `${r.xstate_wasm.startup_ms.mean.toFixed(3)} ms`
      : '—';
    const startup = `${jsStartup} / ${wasmStartup}`;

    const pad = (s, n) => s.padEnd(n);
    md +=
      `| ${pad(r.workload, 8)} | ${pad(jsEps, 14)} | ${pad(wasmEps, 17)} | ` +
      `${pad(speedup, 7)} | ${startup} |\n`;
  }

  return md;
}

main();
