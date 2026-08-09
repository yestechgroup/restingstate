import { readFileSync, writeFileSync, existsSync, readdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const RESULTS_DIR = join(__dirname, 'results');

function loadLatest() {
  const path = join(RESULTS_DIR, 'latest.json');
  if (!existsSync(path)) {
    console.error('No results found. Run `npm run bench` first.');
    process.exit(1);
  }
  return JSON.parse(readFileSync(path, 'utf-8'));
}

function pad(str, len) {
  return str.padEnd(len);
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

  md += '| Workload | xstate.js (eps) | xstate-wasm (eps) | Speedup |';
  md += ' Startup (js/wasm) |\n';
  md += '|----------|----------------|-------------------|---------|-----'
    + '-----------------|\n';

  for (const r of results) {
    const jsEps = r.xstate_js.throughput_eps.mean.toLocaleString();
    const wasmEps = r.xstate_wasm
      ? r.xstate_wasm.throughput_eps.mean.toLocaleString()
      : '—';
    const speedup = r.speedup ? `${r.speedup.toFixed(1)}x` : '—';
    const jsStartup = `${r.xstate_js.startup_ms.mean.toFixed(2)} ms`;
    const wasmStartup = r.xstate_wasm
      ? `${r.xstate_wasm.startup_ms.mean.toFixed(2)} ms`
      : '—';
    const startup = `${jsStartup} / ${wasmStartup}`;

    md += `| ${pad(r.workload, 8)} | ${pad(jsEps, 14)} | ${pad(wasmEps, 17)} | `
      + `${pad(speedup, 7)} | ${startup} |\n`;
  }

  return md;
}

function main() {
  const report = loadLatest();
  const md = generateMarkdown(report);
  const outPath = join(RESULTS_DIR, 'report.md');
  writeFileSync(outPath, md);
  console.log(md);
  console.log(`Report written to ${outPath}`);
}

main();
