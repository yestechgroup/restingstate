#!/usr/bin/env node
/**
 * extract-test-vectors.mjs
 *
 * Parses an xstate.js `.test.ts` file and generates compatibility test
 * vectors for the restingstate compatibility harness.
 *
 * Usage:
 *   node tests/compatibility/extract-test-vectors.mjs <file.test.ts> [more...]
 *
 * For each `it(...)` block the tool:
 *   1. runs the Rust extractor binary on the block text to get its machine IR
 *   2. scans the block for `actor.send({...})` calls -> event sequences
 *   3. scans the block for `expect(...getSnapshot().value).toEqual(...)`
 *      assertions -> expected snapshots
 *   4. writes a test vector to
 *      tests/compatibility/vectors/extracted/<category>/<source>-<index>.json
 */
import { execFileSync, spawnSync } from "child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { basename, dirname, join, resolve } from "path";
import { fileURLToPath } from "url";

const HERE = dirname(fileURLToPath(import.meta.url));
const VECTORS_DIR = join(HERE, "vectors", "extracted");

// ---------------------------------------------------------------------------
// Rust extractor binary
// ---------------------------------------------------------------------------

function findExtractBinary() {
  const built = resolve(process.cwd(), "target/debug/extract");
  if (existsSync(built)) return built;
  console.log("Extract binary not found at target/debug/extract; building...");
  execFileSync("cargo", ["build", "-p", "xstate-parser", "--bin", "extract"], {
    stdio: "inherit",
  });
  if (!existsSync(built)) {
    throw new Error("cargo build -p xstate-parser --bin extract failed");
  }
  return built;
}

function extractMachinesFromSource(binary, sourceText) {
  const tmpDir = mkdtempSync(join(tmpdir(), "xstate-extract-"));
  const tmpFile = join(tmpDir, "block.ts");
  writeFileSync(tmpFile, sourceText);
  try {
    const res = spawnSync(binary, [tmpFile], {
      encoding: "utf8",
      maxBuffer: 64 * 1024 * 1024,
    });
    if (res.status !== 0) return null;
    const parsed = JSON.parse(res.stdout);
    if (parsed && parsed.error) return null;
    return Array.isArray(parsed) ? parsed : null;
  } catch {
    return null;
  } finally {
    rmSync(tmpDir, { recursive: true, force: true });
  }
}

// ---------------------------------------------------------------------------
// Text scanning helpers
// ---------------------------------------------------------------------------

/** Find the index of the `)` matching the `(` at openIdx (string/comment aware). */
function findMatchingParen(source, openIdx) {
  let depth = 0;
  let quote = null;
  let i = openIdx;
  while (i < source.length) {
    const ch = source[i];
    if (quote) {
      if (ch === "\\") {
        i += 2;
        continue;
      }
      if (ch === quote) quote = null;
      i++;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      quote = ch;
      i++;
      continue;
    }
    if (ch === "/" && source[i + 1] === "/") {
      while (i < source.length && source[i] !== "\n") i++;
      continue;
    }
    if (ch === "/" && source[i + 1] === "*") {
      i += 2;
      while (i < source.length && !(source[i] === "*" && source[i + 1] === "/")) i++;
      i += 2;
      continue;
    }
    if (ch === "(") depth++;
    else if (ch === ")") {
      depth--;
      if (depth === 0) return i;
    }
    i++;
  }
  return -1;
}

/** Split text on a separator at bracket depth 0, respecting strings. */
function splitTopLevel(text, sep) {
  const parts = [];
  let depth = 0;
  let quote = null;
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (quote) {
      if (ch === "\\") {
        i++;
        continue;
      }
      if (ch === quote) quote = null;
      continue;
    }
    if (ch === "'" || ch === '"' || ch === "`") {
      quote = ch;
      continue;
    }
    if (ch === "(" || ch === "{" || ch === "[") depth++;
    else if (ch === ")" || ch === "}" || ch === "]") depth--;
    else if (ch === sep && depth === 0) {
      parts.push(text.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(text.slice(start));
  return parts;
}

/** Remove // and /* *\/ comments (newline aware). */
function stripComments(text) {
  let out = "";
  let i = 0;
  while (i < text.length) {
    if (text[i] === "/" && text[i + 1] === "/") {
      while (i < text.length && text[i] !== "\n") i++;
    } else if (text[i] === "/" && text[i + 1] === "*") {
      i += 2;
      while (i < text.length && !(text[i] === "*" && text[i + 1] === "/")) i++;
      i += 2;
    } else {
      out += text[i];
      i++;
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// JS literal -> JSON value
// ---------------------------------------------------------------------------

function parseJsLiteral(rawText) {
  const text = stripComments(rawText).trim();
  if (text === "undefined") return undefined;
  if (text === "null") return null;
  if (text === "true") return true;
  if (text === "false") return false;
  if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(text)) return Number(text);
  const q = text[0];
  if (q === "'" || q === '"' || q === "`") {
    if (text.length < 2 || text[text.length - 1] !== q) return undefined;
    const body = text.slice(1, -1);
    if (q === "`" && body.includes("${")) return undefined;
    return body
      .replace(/\\'/g, "'")
      .replace(/\\"/g, '"')
      .replace(/\\\\/g, "\\")
      .replace(/\\n/g, "\n")
      .replace(/\\t/g, "\t");
  }
  if (text.startsWith("{")) return parseObjectLiteral(text);
  if (text.startsWith("[")) return parseArrayLiteral(text);
  return undefined;
}

function parseObjectLiteral(text) {
  const inner = text.slice(1, -1).trim();
  if (!inner) return {};
  const obj = {};
  for (const part of splitTopLevel(inner, ",")) {
    const m = part.match(/^\s*(?:['"`]([^'"`]+)['"`]|([A-Za-z_$][\w$]*))\s*:\s*([\s\S]*)$/);
    if (!m) continue;
    const key = m[1] !== undefined ? m[1] : m[2];
    const value = parseJsLiteral(m[3]);
    if (value !== undefined) obj[key] = value;
  }
  return obj;
}

function parseArrayLiteral(text) {
  const inner = text.slice(1, -1).trim();
  if (!inner) return [];
  const out = [];
  for (const part of splitTopLevel(inner, ",")) {
    const value = parseJsLiteral(part);
    if (value !== undefined) out.push(value);
  }
  return out;
}

/** Parse the body of a `send({ ... })` object into an event object. */
function parseEventObject(body) {
  const event = {};
  for (const part of splitTopLevel(body, ",")) {
    const m = part.match(/^\s*(['"`]?([A-Za-z_$][\w$]*)['"`]?)\s*:\s*([\s\S]*)$/);
    if (!m) continue;
    const key = m[2];
    if (key === "type") {
      const typeMatch = m[3].trim().match(/^['"`]([^'"`]+)['"`]$/);
      if (typeMatch) event.type = typeMatch[1];
      continue;
    }
    const value = parseJsLiteral(m[3]);
    if (value !== undefined) event[key] = value;
  }
  return event;
}

// ---------------------------------------------------------------------------
// it() block scanner
// ---------------------------------------------------------------------------

const IT_OPEN_RE = /^\s*(?:it|test)(?:\.[A-Za-z_$][\w$]*)?\(\s*/;

/** Read a quoted string starting at idx; returns [content, endIdx] or null. */
function readQuoted(source, idx) {
  const quote = source[idx];
  if (quote !== "'" && quote !== '"' && quote !== "`") return null;
  let i = idx + 1;
  while (i < source.length) {
    if (source[i] === "\\") {
      i += 2;
      continue;
    }
    if (source[i] === quote) return [source.slice(idx + 1, i), i + 1];
    i++;
  }
  return null;
}

function findItBlocks(source) {
  const blocks = [];
  const lines = source.split("\n");
  let offset = 0;
  for (let i = 0; i < lines.length; i++) {
    const lineStart = offset;
    offset += lines[i].length + 1;
    const m = lines[i].match(IT_OPEN_RE);
    if (!m) continue;
    const qIdx = m.index + m[0].length;
    const quoted = readQuoted(lines[i], qIdx);
    if (!quoted) continue;
    const [name, afterQuote] = quoted;
    let j = afterQuote;
    while (j < lines[i].length && /\s/.test(lines[i][j])) j++;
    if (lines[i][j] !== ",") continue;
    const openIdx = lineStart + lines[i].indexOf("(");
    const closeIdx = findMatchingParen(source, openIdx);
    if (closeIdx < 0) continue;
    const start = lineStart;
    const text = source.slice(start, closeIdx + 1);
    if (blocks.some((b) => start >= b.start && start < b.end)) continue;
    blocks.push({ name, start, end: closeIdx + 1, text });
  }
  return blocks;
}

// ---------------------------------------------------------------------------
// Event / expected extraction
// ---------------------------------------------------------------------------

const SEND_RE = /(?:actorRef|actor|service|childRef|ref|spawnedRef)\.send\(\s*\{([\s\S]*?)\}\s*\)/g;

function scanEvents(blockText) {
  const events = [];
  let m;
  while ((m = SEND_RE.exec(blockText)) !== null) {
    const event = parseEventObject(m[1]);
    if (event && typeof event.type === "string" && event.type.length > 0) {
      events.push({ pos: m.index, event });
    }
  }
  return events;
}

const EXPECT_RE = /\bexpect\s*\(/g;
const ASSERT_METHOD_RE = /^\.(?:toEqual|toBe|toStrictEqual|toMatchObject)\s*\(/;

function scanExpectedValues(blockText) {
  const expects = [];
  EXPECT_RE.lastIndex = 0;
  let m;
  while ((m = EXPECT_RE.exec(blockText)) !== null) {
    const openIdx = blockText.indexOf("(", m.index);
    const closeIdx = findMatchingParen(blockText, openIdx);
    if (closeIdx < 0) continue;
    const arg = blockText.slice(openIdx + 1, closeIdx).trim();
    if (!/\.value\s*(?:!| as [\w.]+)?\s*$/.test(arg)) continue;
    const rest = blockText.slice(closeIdx + 1);
    const am = rest.match(ASSERT_METHOD_RE);
    if (!am) continue;
    const assertOpen = closeIdx + 1 + am[0].indexOf("(");
    const assertClose = findMatchingParen(blockText, assertOpen);
    if (assertClose < 0) continue;
    const literal = blockText.slice(assertOpen + 1, assertClose);
    const value = parseJsLiteral(literal);
    if (value === undefined) continue;
    expects.push({ pos: m.index, value });
    EXPECT_RE.lastIndex = assertClose + 1;
  }
  return expects;
}

// ---------------------------------------------------------------------------
// Skip rules
// ---------------------------------------------------------------------------

const SKIP_PATTERNS = [
  [/trackEntries\s*\(/, "trackEntries"],
  [/\bassign\s*\(/, "assign"],
  [/guard\s*:\s*\(/, "inline guards"],
  [/guard\s*:\s*[A-Za-z_$][\w$]*\s*=>/, "inline guards"],
  [/\bvi\.fn\s*\(/, "vi.fn mocks"],
  [/\braise\s*\(/, "raise"],
  [/\bsendTo\s*\(/, "sendTo"],
  [/\bsendParent\s*\(/, "sendParent"],
  [/\bspawn\s*\(/, "spawn"],
  [/getSnapshot\(\)\.children\.[A-Za-z_$][\w$]*\.send\(/, "child actor send"],
  [/\b(?:initialTransition|transition)\s*\(/, "transition helpers"],
  [/type\s*:\s*['"`]history['"`][^}]*?target\s*:/, "history with target"],
  // Absolute state-node targets (`target: '#x'` or shorthand `EVENT: '#x'`).
  // `to:`/`type:`/`id:`/`route:`/`initial:` values are not state targets.
  [/[\s{};,](?!to\s*:|route\s*:|type\s*:|id\s*:|initial\s*:)[A-Za-z_$][\w$]*\s*:\s*['"`]#[^'"`]+['"`]/, "absolute targets"],
  [/\bafter\s*:/, "after (timing)"],
  // `onDone` on a state node (parallel completion) is not extracted.
  [/\bonDone\s*:/, "onDone (final completion)"],
];

function findSkipReason(blockText) {
  for (const [re, reason] of SKIP_PATTERNS) {
    if (re.test(blockText)) return reason;
  }
  return null;
}

// ---------------------------------------------------------------------------
// IR -> machine_config (mirrors ts-runner.mjs)
// ---------------------------------------------------------------------------

function buildStateNamePathMap(states, prefix = "") {
  const map = {};
  for (const state of states) {
    const path = prefix ? `${prefix}.${state.name}` : state.name;
    if (!(state.name in map)) map[state.name] = [];
    map[state.name].push(path);
    if (state.children && state.children.length > 0) {
      // Merge instead of overwrite: duplicate names must keep every path so
      // the uniqueness check below stays correct.
      const childMap = buildStateNamePathMap(state.children, path);
      for (const [name, paths] of Object.entries(childMap)) {
        map[name] = [...(map[name] || []), ...paths];
      }
    }
  }
  return map;
}

/** Map of state-node path -> direct child paths (used for relative targets). */
function buildChildPathMap(states, prefix = "", map = new Map()) {
  for (const state of states) {
    const path = prefix ? `${prefix}.${state.name}` : state.name;
    if (!map.has(prefix)) map.set(prefix, []);
    map.get(prefix).push(path);
    if (!map.has(path)) map.set(path, []);
    if (state.children && state.children.length > 0) {
      for (const child of state.children) {
        buildChildPathMap([child], path, map);
      }
    }
  }
  return map;
}

function irToXstateConfig(ir) {
  const machineId = ir.id || "machine";
  const namePathMap = buildStateNamePathMap(ir.states, machineId);
  const childPathMap = buildChildPathMap(ir.states, machineId);

  function resolveTarget(target, nodePath) {
    if (!target) return target;
    // Resolve relative targets the way xstate.js does: walk up from the
    // source node's scope and take the nearest ancestor whose descendant
    // matches. Handles plain names and dot-paths like `on.hist`.
    const parts = target.split(".");
    const scopeSegments = nodePath.split(".");
    for (let len = scopeSegments.length; len >= 1; len--) {
      let current = scopeSegments.slice(0, len).join(".");
      let found = true;
      for (const part of parts) {
        const kids = childPathMap.get(current);
        const next = kids && kids.find((p) => p.split(".").pop() === part);
        if (!next) {
          found = false;
          break;
        }
        current = next;
      }
      if (found) return `#${current}`;
    }
    // Fall back to unambiguous global names; ambiguous names are resolved
    // relative to the state node by xstate.js itself.
    const paths = namePathMap[target];
    if (paths && paths.length === 1) return `#${paths[0]}`;
    return target;
  }

  function convertState(state, path) {
    const result = { id: path };
    if (state.type === "deephistory") {
      result.type = "history";
      result.history = "deep";
    } else if (state.type && state.type !== "atomic") {
      result.type = state.type;
    }
    if (state.entry && state.entry.length > 0) result.entry = state.entry;
    if (state.exit && state.exit.length > 0) result.exit = state.exit;
    if (state.tags && state.tags.length > 0) result.tags = state.tags;

    if (state.transitions && state.transitions.length > 0) {
      const on = {};
      const alwaysTransitions = [];
      for (const t of state.transitions) {
        const isAlways = t.event === "" || t.event === undefined;
        const entry = {};
        if (t.target) entry.target = resolveTarget(t.target, path);
        if (t.guard) entry.guard = t.guard.name;
        if (t.actions && t.actions.length > 0) entry.actions = t.actions.map((a) => a.name);
        if (isAlways) {
          alwaysTransitions.push(entry);
        } else {
          if (!on[t.event]) on[t.event] = [];
          on[t.event].push(entry);
        }
      }
      for (const [event, transitions] of Object.entries(on)) {
        if (transitions.length === 1) {
          const t = transitions[0];
          if (!t.guard && !t.actions) {
            on[event] = t.target || undefined;
          } else if (!t.actions) {
            on[event] = { target: t.target, guard: t.guard };
          }
        }
      }
      result.on = on;
      if (alwaysTransitions.length > 0) result.always = alwaysTransitions;
    }

    if (state.children && state.children.length > 0) {
      result.states = {};
      for (const child of state.children) {
        const childPath = `${path}.${child.name}`;
        result.states[child.name] = convertState(child, childPath);
      }
      if (state.initial) result.initial = state.initial;
    }

    if (state.invoke) {
      result.invoke = {
        src: state.invoke.src,
        id: state.invoke.id,
        ...(state.invoke.on_done && state.invoke.on_done.length > 0
          ? { onDone: state.invoke.on_done[0].target }
          : {}),
        ...(state.invoke.on_error && state.invoke.on_error.length > 0
          ? { onError: state.invoke.on_error[0].target }
          : {}),
      };
    }
    return result;
  }

  const config = { id: machineId, initial: ir.initial };
  if (ir.context && ir.context.fields && ir.context.fields.length > 0) {
    config.context = {};
    for (const field of ir.context.fields) {
      config.context[field.name] = field.default_value;
    }
  } else {
    config.context = {};
  }
  config.states = {};
  for (const state of ir.states) {
    const statePath = `${machineId}.${state.name}`;
    config.states[state.name] = convertState(state, statePath);
  }
  return config;
}

// ---------------------------------------------------------------------------
// Initial state resolution
// ---------------------------------------------------------------------------

function resolveInitialStateValue(ir) {
  // History states are transparent: they never appear in a state value.
  const nonHistory = (states) =>
    (states || []).filter((s) => s.type !== "history" && s.type !== "deephistory");

  const walk = (states, initialName) => {
    const list = nonHistory(states);
    if (list.length === 0) return null;
    const name = initialName && list.some((s) => s.name === initialName) ? initialName : list[0].name;
    const node = list.find((s) => s.name === name);
    if (!node) return name;
    if (node.type === "parallel") {
      const regions = {};
      // Regions are the node's own children, not its siblings.
      for (const child of node.children || []) {
        regions[child.name] = walk(child.children, child.initial) ?? child.name;
      }
      return { [node.name]: regions };
    }
    if (node.type === "compound" || (node.children && node.children.length > 0)) {
      const childValue = walk(node.children, node.initial);
      return childValue === null ? node.name : { [node.name]: childValue };
    }
    return node.name;
  };

  const rootIsParallel =
    !ir.initial && ir.states.length > 1 && ir.states.every((s) => s.children && s.children.length > 0);
  if (rootIsParallel) {
    const regions = {};
    for (const child of nonHistory(ir.states)) {
      regions[child.name] = walk(child.children, child.initial) ?? child.name;
    }
    return regions;
  }
  return walk(ir.states, ir.initial || null);
}

// ---------------------------------------------------------------------------
// Category detection
// ---------------------------------------------------------------------------

function detectCategory(filePath) {
  const base = basename(filePath).toLowerCase();
  const rules = [
    [/transition/, "simple"],
    [/state/, "simple"],
    [/deep/, "compound"],
    [/parallel/, "parallel"],
    [/final/, "final"],
    [/guard/, "guards"],
    [/transient/, "always"],
    [/always/, "always"],
    [/history/, "history"],
    [/after/, "after"],
    [/invoke/, "invoke"],
    [/initial/, "initial"],
  ];
  for (const [re, category] of rules) {
    if (re.test(base)) return category;
  }
  return "simple";
}

// ---------------------------------------------------------------------------
// Vector construction
// ---------------------------------------------------------------------------

/** True when two plain objects have the same top-level key sets. */
function sameKeySets(a, b) {
  if (
    a === null ||
    b === null ||
    typeof a !== "object" ||
    typeof b !== "object" ||
    Array.isArray(a) ||
    Array.isArray(b)
  ) {
    return JSON.stringify(a) === JSON.stringify(b);
  }
  return JSON.stringify(Object.keys(a).sort()) === JSON.stringify(Object.keys(b).sort());
}

function buildInitialContext(machineIr) {
  const ctx = {};
  if (machineIr && machineIr.context && machineIr.context.fields) {
    for (const field of machineIr.context.fields) {
      ctx[field.name] = field.default_value;
    }
  }
  return ctx;
}

function buildVector({ block, machineIr, fileBase, index, skipCompare }) {
  const events = scanEvents(block.text);
  const expects = scanExpectedValues(block.text);

  const items = [
    ...events.map((e) => ({ kind: "event", ...e })),
    ...expects.map((e) => ({ kind: "expect", ...e })),
  ].sort((a, b) => a.pos - b.pos);

  let initialValue;
  const initialFromAssertion = [];
  const expectedByStep = {};
  let lastEventStep = -1;

  for (const item of items) {
    if (item.kind === "event") {
      lastEventStep++;
      continue;
    }
    if (lastEventStep < 0) {
      initialFromAssertion.push(item.value);
    } else {
      expectedByStep[lastEventStep] = item.value;
    }
  }

  let degradedInitial = false;
  const resolvedInitial = machineIr ? resolveInitialStateValue(machineIr) : null;
  if (initialFromAssertion.length > 0) {
    initialValue = initialFromAssertion[0];
    // If the test asserts a value the extracted IR cannot reproduce (e.g.
    // regions of a parallel root, which the IR cannot express), fall back to
    // the machine-resolved value so both runtimes agree.
    if (resolvedInitial !== null && !sameKeySets(resolvedInitial, initialValue)) {
      initialValue = resolvedInitial;
      degradedInitial = true;
    }
  } else {
    initialValue = resolvedInitial;
  }

  if (!machineIr) return null;

  const machineName = /^machine-\d+$/.test(machineIr.id)
    ? `${fileBase}-${index}`
    : machineIr.id;

  const expected = [];
  const initialContext = buildInitialContext(machineIr);
  for (let i = 0; i < events.length; i++) {
    if (i in expectedByStep) {
      expected.push({
        step: i,
        event: events[i].event,
        snapshot: { value: expectedByStep[i], context: initialContext },
      });
    }
  }

  const sendMatches = [...block.text.matchAll(SEND_RE)].length;
  const allEventsParsed = sendMatches === events.length;
  const full =
    !skipCompare &&
    !degradedInitial &&
    allEventsParsed &&
    (events.length === 0 || (expected.length > 0 && expected.length >= events.length));

  return {
    machine_name: machineName,
    machine_ir: machineIr,
    machine_config: irToXstateConfig(machineIr),
    protocol: {
      initial: { value: initialValue, context: initialContext },
      events: events.map((e) => e.event),
    },
    expected,
    ...(skipCompare ? { skip_compare: true } : {}),
    extraction_status: full ? "full" : "partial",
  };
}

// ---------------------------------------------------------------------------
// Per-file processing
// ---------------------------------------------------------------------------

function processFile(filePath, binary) {
  const source = readFileSync(filePath, "utf-8");
  const fileBase = basename(filePath).replace(/\.test\.ts$/, "").replace(/\.ts$/, "");
  const category = detectCategory(filePath);
  const outDir = join(VECTORS_DIR, category);
  mkdirSync(outDir, { recursive: true });

  const blocks = findItBlocks(source);
  const vectors = [];
  const skipped = [];
  let machinesFound = 0;

  for (const block of blocks) {
    try {
      const skipReason = findSkipReason(block.text);
      if (skipReason) {
        skipped.push({ block, reason: skipReason });
        continue;
      }

      const machines = extractMachinesFromSource(binary, block.text);
      if (!machines || machines.length === 0) {
        skipped.push({ block, reason: "no machine found" });
        continue;
      }
      machinesFound += machines.length;
      // Blocks may define several machines (e.g. a child + a parent). The
      // assertions in xstate tests usually target the last one created.
      const machineIr = machines[machines.length - 1];
      if (!machineIr.states || machineIr.states.length === 0) {
        // The parser failed to extract any state (e.g. `invoke` with a
        // function expression). The IR is unusable.
        skipped.push({ block, reason: "machine with no states" });
        continue;
      }
      if (!machineIr.initial) {
        if (scanEvents(block.text).length > 0) {
          // The IR cannot express a parallel root (no root-level `type`), so
          // event-driven tests on such machines cannot be reproduced faithfully.
          skipped.push({ block, reason: "root parallel" });
          continue;
        }
        // Initial-only root-parallel machines: degrade to the first region so
        // both runtimes agree (the vector is marked partial).
        if (machineIr.states.length > 0) {
          machineIr.initial = machineIr.states[0].name;
        }
      }

      const events = scanEvents(block.text);
      const expects = scanExpectedValues(block.text);
      if (events.length === 0 && expects.length === 0) {
        skipped.push({ block, reason: "no events found" });
        continue;
      }
      if (expects.length === 0) {
        skipped.push({ block, reason: "no expected values found" });
        continue;
      }

      const skipCompare = /after\s*:/.test(block.text);
      const vector = buildVector({ block, machineIr, fileBase, index: vectors.length, skipCompare });
      if (!vector) {
        skipped.push({ block, reason: "vector build failed" });
        continue;
      }
      vectors.push(vector);

      const outFile = join(outDir, `${fileBase}-${vectors.length - 1}.json`);
      writeFileSync(outFile, JSON.stringify(vector, null, 2) + "\n");
      console.log(`  -> ${outFile}`);
    } catch (err) {
      skipped.push({ block, reason: `error: ${err.message}` });
    }
  }

  const skipCounts = {};
  for (const s of skipped) {
    skipCounts[s.reason] = (skipCounts[s.reason] || 0) + 1;
  }

  console.log(`\nProcessing: ${basename(filePath)}`);
  console.log(`Machines found: ${machinesFound}`);
  console.log(`it() blocks: ${blocks.length}`);
  console.log(`Vectors generated: ${vectors.length}`);
  console.log(`Skipped: ${skipped.length}`);
  for (const [reason, count] of Object.entries(skipCounts)) {
    console.log(`  - ${count} ${reason}`);
  }
  console.log(`Output: ${outDir}/`);
  return { vectors, skipped };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

function main() {
  const files = process.argv.slice(2);
  if (files.length === 0) {
    console.error("Usage: node extract-test-vectors.mjs <test-file.test.ts> [more...]");
    process.exit(1);
  }

  let binary;
  try {
    binary = findExtractBinary();
  } catch (err) {
    console.error(`Failed to build extract binary: ${err.message}`);
    process.exit(1);
  }

  for (const file of files) {
    if (!existsSync(file)) {
      console.error(`File not found: ${file}`);
      continue;
    }
    processFile(file, binary);
  }
}

main();
