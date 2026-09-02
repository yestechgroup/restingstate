// Tests for scripts/assemble-npm.mjs (issue #32). Run: node --test scripts/
//
// Fixtures simulate wasm-pack output dirs in a temp dir. The CJS-flavored
// node fixture mirrors what wasm-bindgen 0.2.127 `--target nodejs` actually
// emits (verified: `exports.X = ...` + `require('fs')`); the ESM-flavored one
// exercises the issue's literal exports-map shape.

import assert from 'node:assert/strict';
import { test } from 'node:test';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { assembleNpm, main, parseArgs } from './assemble-npm.mjs';

const CJS_NODE_GLUE = [
  "const wasmBytes = require('fs').readFileSync(wasmPath);",
  'function init() {}',
  'exports.init = init;',
  'module.exports = { init };',
  '',
].join('\n');
const ESM_NODE_GLUE = 'export default function init() {}\n';

function makeFixture(root, { nodeGlue = CJS_NODE_GLUE } = {}) {
  const bundler = join(root, 'pkg-bundler');
  const node = join(root, 'pkg-node');
  mkdirSync(bundler, { recursive: true });
  mkdirSync(node, { recursive: true });
  writeFileSync(join(bundler, 'xstate_wasm.js'), 'import * as wasm from "./xstate_wasm_bg.wasm";\nexport default init;\n');
  writeFileSync(join(bundler, 'xstate_wasm_bg.wasm'), 'wasm-bytes-bundler');
  writeFileSync(join(bundler, 'xstate_wasm.d.ts'), 'export default function init(): Promise<void>;\n');
  writeFileSync(join(bundler, 'package.json'), '{"name":"xstate-wasm","version":"0.0.0"}');
  writeFileSync(join(bundler, '.gitignore'), '*\n');
  writeFileSync(join(node, 'xstate_wasm.js'), nodeGlue);
  writeFileSync(join(node, 'xstate_wasm_bg.wasm'), 'wasm-bytes-node');
  writeFileSync(join(node, 'xstate_wasm.d.ts'), 'export function init(): Promise<void>;\n');
  writeFileSync(join(node, 'package.json'), '{"name":"xstate-wasm","version":"0.0.0"}');
  writeFileSync(join(node, '.gitignore'), '*\n');
  return { bundler, node };
}

function withTemp(name, fn) {
  return async (t) => {
    const root = mkdtempSync(join(tmpdir(), `assemble-npm-${name}-`));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    await fn(root);
  };
}

test('version injection: generated package.json carries the --version arg', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-version-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');

  const { packageJson } = assembleNpm({ version: '1.2.3-rc.1', bundler, node, out });

  assert.equal(packageJson.version, '1.2.3-rc.1');
  assert.equal(JSON.parse(readFileSync(join(out, 'package.json'), 'utf8')).version, '1.2.3-rc.1');
});

test('exports map exact shape (issue spec: node/default conditions + ./* passthrough)', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-exports-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  // ESM node glue keeps the literal spec path `./pkg-node/xstate_wasm.js`.
  const { bundler, node } = makeFixture(join(root, 'in'), { nodeGlue: ESM_NODE_GLUE });

  const { packageJson } = assembleNpm({ version: '0.2.0', bundler, node, out: join(root, 'dist-npm') });

  assert.deepEqual(packageJson.exports, {
    '.': {
      node: './pkg-node/xstate_wasm.js',
      default: './pkg-bundler/xstate_wasm.js',
    },
    './*': './*',
  });
});

test('CJS nodejs glue (what wasm-bindgen 0.2.127 emits) is remapped to .cjs inside a type:module package', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-cjs-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in')); // default fixture is CJS
  const out = join(root, 'dist-npm');

  const { packageJson, nodeEntry } = assembleNpm({ version: '0.2.0', bundler, node, out });

  assert.equal(packageJson.exports['.'].node, './pkg-node/xstate_wasm.cjs');
  assert.equal(nodeEntry, './pkg-node/xstate_wasm.cjs');
  assert.ok(existsSync(join(out, 'pkg-node', 'xstate_wasm.cjs')), '.cjs entry exists');
  assert.ok(!existsSync(join(out, 'pkg-node', 'xstate_wasm.js')), 'stray .js CJS entry removed');
  assert.equal(packageJson.type, 'module');
});

test('both pkg dirs are copied into the output', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-copy-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');

  assembleNpm({ version: '0.2.0', bundler, node, out });

  for (const dir of ['pkg-bundler', 'pkg-node']) {
    for (const entry of ['xstate_wasm_bg.wasm', 'xstate_wasm.d.ts']) {
      assert.ok(existsSync(join(out, dir, entry)), `${dir}/${entry} present`);
    }
  }
  assert.ok(existsSync(join(out, 'pkg-bundler', 'xstate_wasm.js')), 'bundler ESM entry present');
});

test('.d.ts present in both dirs and types field points at the bundler build', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-dts-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');

  const { packageJson } = assembleNpm({ version: '0.2.0', bundler, node, out });

  assert.equal(packageJson.types, './pkg-bundler/xstate_wasm.d.ts');
  assert.equal(packageJson.main, './pkg-bundler/xstate_wasm.js');
  assert.ok(existsSync(join(out, 'pkg-bundler', 'xstate_wasm.d.ts')));
  assert.ok(existsSync(join(out, 'pkg-node', 'xstate_wasm.d.ts')));
});

test('generated package.json has license, repository (yestechgroup), engines.node >= 18, name, files', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-meta-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));

  const { packageJson } = assembleNpm({ version: '0.2.0', bundler, node, out: join(root, 'dist-npm') });

  assert.equal(packageJson.name, '@yestechgroup/wasm');
  assert.equal(packageJson.license, 'MIT OR Apache-2.0');
  assert.deepEqual(packageJson.repository, { type: 'git', url: 'https://github.com/yestechgroup/restingstate' });
  assert.deepEqual(packageJson.engines, { node: '>=18' });
  assert.deepEqual(packageJson.files, ['pkg-bundler/', 'pkg-node/']);
});

test('wasm-pack intermediate package.json and .gitignore files are stripped from both pkg dirs', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-strip-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');

  assembleNpm({ version: '0.2.0', bundler, node, out });

  assert.ok(!existsSync(join(out, 'pkg-bundler', 'package.json')), 'bundler package.json stripped');
  assert.ok(!existsSync(join(out, 'pkg-node', 'package.json')), 'node package.json stripped');
  // wasm-pack writes a lone `*` .gitignore per pkg dir; npm falls back to
  // gitignore rules, so leaving it would exclude the whole dir from npm pack.
  assert.ok(!existsSync(join(out, 'pkg-bundler', '.gitignore')), 'bundler .gitignore stripped');
  assert.ok(!existsSync(join(out, 'pkg-node', '.gitignore')), 'node .gitignore stripped');
  assert.ok(existsSync(join(out, 'package.json')), 'root package.json generated');
});

test('rejects missing inputs: absent dirs, incomplete wasm-pack output, missing/invalid version, unknown flag', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-reject-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');
  const missingDir = join(root, 'nope');

  assert.throws(() => assembleNpm({ version: '0.2.0', bundler: missingDir, node, out }), /--bundler dir not found/);
  assert.throws(() => assembleNpm({ version: '0.2.0', bundler, node: missingDir, out }), /--node dir not found/);
  assert.throws(() => assembleNpm({ version: '0.2.0', bundler: '', node, out }), /missing --bundler/);
  assert.throws(() => assembleNpm({ version: '', bundler, node, out }), /missing --version/);
  assert.throws(() => assembleNpm({ version: 'v0.2.0', bundler, node, out }), /invalid --version/);

  const incomplete = join(root, 'incomplete');
  mkdirSync(incomplete, { recursive: true });
  writeFileSync(join(incomplete, 'xstate_wasm.js'), 'export default 0;\n');
  assert.throws(
    () => assembleNpm({ version: '0.2.0', bundler, node: incomplete, out }),
    /missing wasm-pack output file\(s\): xstate_wasm_bg\.wasm, xstate_wasm\.d\.ts/,
  );

  assert.throws(() => parseArgs(['--bogus', 'x']), /unknown option: --bogus/);
  assert.throws(() => parseArgs(['--version']), /missing value for --version/);

  assert.throws(
    () => assembleNpm({ version: '0.2.0', bundler, node, out: bundler }),
    /--out must differ from the input dirs/,
  );
});

test('reruns are idempotent: a previous out dir is replaced wholesale', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-idem-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');

  assembleNpm({ version: '0.2.0', bundler, node, out });
  writeFileSync(join(out, 'stale.txt'), 'stale');
  assembleNpm({ version: '0.2.1', bundler, node, out });

  assert.ok(!existsSync(join(out, 'stale.txt')), 'stale file gone');
  assert.equal(JSON.parse(readFileSync(join(out, 'package.json'), 'utf8')).version, '0.2.1');
});

test('parseArgs defaults out to dist-npm and reads all flags', () => {
  assert.deepEqual(parseArgs(['--version', '0.2.0', '--bundler', 'b', '--node', 'n']), {
    version: '0.2.0', bundler: 'b', node: 'n', out: 'dist-npm',
  });
  assert.equal(parseArgs(['--help']).help, true);
});

test('main() exit codes: 0 on success/help, 1 on bad input (verified via a real subprocess-free run in tmp cwd)', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'assemble-npm-main-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const { bundler, node } = makeFixture(join(root, 'in'));
  const out = join(root, 'dist-npm');

  assert.equal(main(['--version', '0.2.0', '--bundler', bundler, '--node', node, '--out', out]), 0);
  assert.equal(main(['--help']), 0);
  assert.equal(main(['--version', 'nope', '--bundler', bundler, '--node', node, '--out', out]), 1);
  assert.equal(main(['--version', '0.2.0', '--bundler', join(root, 'gone'), '--node', node, '--out', out]), 1);
});
