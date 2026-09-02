#!/usr/bin/env node
// Assemble the publishable npm package from wasm-pack outputs (issue #32).
//
// Inputs: two wasm-pack output dirs plus a version string.
//   pkg-bundler/  — `wasm-pack build --target bundler --out-dir pkg-bundler` (ESM)
//   pkg-node/     — `wasm-pack build --target nodejs  --out-dir pkg-node`    (CJS)
// Output: dist-npm/ — a publishable package whose root package.json is generated
// here (never copied from crates/xstate-wasm): the script's contract is
// "wasm-pack outputs + version arg", nothing else.
//
// Known wasm-bindgen 0.2.127 facts this script handles (verified against the
// pinned CLI, see scripts/assemble-npm.test.mjs):
//   * `--target nodejs` glue is CommonJS (`exports.X = ...`, `require('fs')`).
//     Inside a `"type": "module"` package Node would parse that .js as ESM and
//     crash, so the entry is copied to `xstate_wasm.cjs` and the `node` export
//     condition points there. Genuinely-ESM glue keeps the plain `.js` path.
//   * wasm-pack writes an intermediate package.json (no "type" field — would
//     override the root type for everything inside the dir, flipping the ESM
//     bundler glue back to CJS parsing) and a per-dir `.gitignore` (a lone
//     `*` — with no .npmignore, npm honors gitignore rules, so `npm pack`
//     would ship ONLY the root package.json) into each out-dir. Both are
//     intermediate artifacts, so they are stripped.

import { cpSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PKG_NAME = 'restingstate';
const PKG_DESCRIPTION = 'XState-compatible state machines in WebAssembly';
const PKG_LICENSE = 'MIT OR Apache-2.0';
const PKG_REPOSITORY_URL = 'https://github.com/yestechgroup/restingstate';
const PKG_KEYWORDS = ['xstate', 'wasm', 'state-machine', 'actor', 'statechart'];
// README shipped in the tarball — npm renders the package page from it.
const NPM_README_SRC = join(dirname(fileURLToPath(import.meta.url)), 'npm-README.md');

const ENTRY_JS = 'xstate_wasm.js';
const ENTRY_BG_WASM = 'xstate_wasm_bg.wasm';
const ENTRY_D_TS = 'xstate_wasm.d.ts';
const REQUIRED_ENTRIES = [ENTRY_JS, ENTRY_BG_WASM, ENTRY_D_TS];

// Loosely a SemVer; the real gate is npm itself. Blocks accidental prose like
// `v0.2.0` (tag form) or `0.2` from reaching the registry.
const SEMVER_RE = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

const USAGE = `usage: node scripts/assemble-npm.mjs --version <semver> --bundler <dir> --node <dir> [--out <dir>]

  --version <semver>  package version (e.g. 0.2.0; a release workflow passes the git tag with 'v' stripped)
  --bundler <dir>     wasm-pack --target bundler output dir
  --node <dir>        wasm-pack --target nodejs output dir
  --out <dir>         output package dir (default: dist-npm)`;

export function parseArgs(argv) {
  const opts = { version: '', bundler: '', node: '', out: 'dist-npm' };
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--help') {
      opts.help = true;
      return opts;
    }
    const value = argv[++i];
    if (value === undefined) throw new Error(`missing value for ${flag}\n\n${USAGE}`);
    switch (flag) {
      case '--version': opts.version = value; break;
      case '--bundler': opts.bundler = value; break;
      case '--node': opts.node = value; break;
      case '--out': opts.out = value; break;
      default: throw new Error(`unknown option: ${flag}\n\n${USAGE}`);
    }
  }
  return opts;
}

function requirePkgDir(kind, dir) {
  if (!existsSync(dir) || !statSync(dir).isDirectory()) {
    throw new Error(`--${kind} dir not found: ${dir} (expected a wasm-pack output dir)`);
  }
  const missing = REQUIRED_ENTRIES.filter((entry) => !existsSync(join(dir, entry)));
  if (missing.length > 0) {
    throw new Error(`--${kind} dir ${dir} is missing wasm-pack output file(s): ${missing.join(', ')}`);
  }
}

// The wasm-bindgen 0.2.127 `--target nodejs` glue is CommonJS; detect the
// markers instead of trusting the target flag so the remap is self-evidencing.
function looksLikeCommonJs(source) {
  return /\bmodule\.exports\b/.test(source) || /(^|\n)\s*exports\.[A-Za-z_$]/.test(source) || /\brequire\s*\(/.test(source);
}

export function assembleNpm(options) {
  const { version, bundler, node, out } = options;

  if (options.help) throw new Error(USAGE);
  if (!version) throw new Error('missing --version\n\n' + USAGE);
  if (!bundler) throw new Error('missing --bundler\n\n' + USAGE);
  if (!node) throw new Error('missing --node\n\n' + USAGE);
  if (!SEMVER_RE.test(version)) throw new Error(`invalid --version '${version}' (expected semver, e.g. 0.2.0)`);

  if (!existsSync(NPM_README_SRC)) throw new Error(`npm README not found: ${NPM_README_SRC}`);
  requirePkgDir('bundler', bundler);
  requirePkgDir('node', node);
  if (existsSync(out) && !statSync(out).isDirectory()) {
    throw new Error(`--out exists and is not a directory: ${out}`);
  }
  // rmSync(out) is part of assembly; refuse inputs that would be destroyed.
  if (out === bundler || out === node) {
    throw new Error(`--out must differ from the input dirs (got --out ${out})`);
  }

  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });

  const readmeDest = join(out, 'README.md');
  cpSync(NPM_README_SRC, readmeDest);

  const bundlerDest = join(out, 'pkg-bundler');
  const nodeDest = join(out, 'pkg-node');
  cpSync(bundler, bundlerDest, { recursive: true });
  cpSync(node, nodeDest, { recursive: true });

  // Strip wasm-pack's intermediate package.json from both dirs: they lack
  // "type", and Node picks the nearest package.json to decide module format —
  // that would flip the ESM bundler glue back to CJS parsing inside the package.
  //
  // Also strip wasm-pack's per-dir `.gitignore` (a lone `*`): with no
  // .npmignore, npm falls back to gitignore rules, and those files would
  // exclude the ENTIRE pkg dir from `npm pack`/`npm publish` (verified with
  // npm 9.2: tarball shrinks to just the root package.json).
  for (const dest of [bundlerDest, nodeDest]) {
    rmSync(join(dest, 'package.json'), { force: true });
    rmSync(join(dest, '.gitignore'), { force: true });
  }

  // Remap CJS nodejs glue to .cjs so it is parseable inside a type:module package.
  const nodeEntrySource = readFileSync(join(nodeDest, ENTRY_JS), 'utf8');
  let nodeEntry = `./pkg-node/${ENTRY_JS}`;
  if (looksLikeCommonJs(nodeEntrySource)) {
    renameSync(join(nodeDest, ENTRY_JS), join(nodeDest, 'xstate_wasm.cjs'));
    nodeEntry = './pkg-node/xstate_wasm.cjs';
  }

  const packageJson = {
    name: PKG_NAME,
    description: PKG_DESCRIPTION,
    version,
    type: 'module',
    main: `./pkg-bundler/${ENTRY_JS}`,
    types: `./pkg-bundler/${ENTRY_D_TS}`,
    exports: {
      '.': {
        node: nodeEntry,
        default: `./pkg-bundler/${ENTRY_JS}`,
      },
      './*': './*',
    },
    files: ['pkg-bundler/', 'pkg-node/'],
    keywords: PKG_KEYWORDS,
    license: PKG_LICENSE,
    repository: {
      type: 'git',
      url: PKG_REPOSITORY_URL,
    },
    engines: {
      node: '>=18',
    },
  };

  writeFileSync(join(out, 'package.json'), JSON.stringify(packageJson, null, 2) + '\n');

  return { outDir: out, packageJson, nodeEntry };
}

export function main(argv) {
  try {
    const opts = parseArgs(argv);
    if (opts.help) {
      console.log(USAGE);
      return 0;
    }
    const { outDir, packageJson, nodeEntry } = assembleNpm(opts);
    console.log(`assembled ${packageJson.name}@${packageJson.version} -> ${outDir}`);
    console.log(`  exports['.'].node    = ${nodeEntry}`);
    console.log(`  exports['.'].default = ${packageJson.exports['.'].default}`);
    return 0;
  } catch (err) {
    console.error(`error: ${err.message}`);
    return 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exit(main(process.argv.slice(2)));
}
