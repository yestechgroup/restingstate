import { readFileSync } from "fs";
import { createMachine, createActor, fromCallback, assign } from "xstate";

function getDottedPath(obj, dottedPath) {
  let cur = obj;
  for (const part of dottedPath.split(".")) {
    if (cur === null || typeof cur !== "object") return undefined;
    cur = cur[part];
  }
  return cur;
}

function setDottedPath(obj, dottedPath, value) {
  const parts = dottedPath.split(".");
  let cur = obj;
  for (let i = 0; i < parts.length - 1; i++) {
    if (cur === null || typeof cur !== "object") return;
    if (cur[parts[i]] === null || typeof cur[parts[i]] !== "object") {
      cur[parts[i]] = {};
    }
    cur = cur[parts[i]];
  }
  if (cur !== null && typeof cur === "object") {
    cur[parts[parts.length - 1]] = value;
  }
}

function applyAssignOps(ops, context, event) {
  const draft = structuredClone(context);
  for (const op of ops) {
    if (!op || typeof op !== "object") continue;
    if (op.op === "set") {
      if (typeof op.from === "string") {
        const dot = op.from.indexOf(".");
        if (dot < 0) continue;
        const root = op.from.slice(0, dot);
        const path = op.from.slice(dot + 1);
        const source = root === "event" ? event : root === "context" ? draft : undefined;
        if (source === undefined) continue;
        const resolved = getDottedPath(source, path);
        if (resolved === undefined) {
          if ("default" in op) setDottedPath(draft, op.field, structuredClone(op.default));
        } else {
          setDottedPath(draft, op.field, structuredClone(resolved));
        }
      } else if ("value" in op) {
        setDottedPath(draft, op.field, structuredClone(op.value));
      }
    } else if (op.op === "add" || op.op === "sub") {
      const current = getDottedPath(draft, op.field);
      const base =
        typeof current === "number"
          ? current
          : "default" in op && typeof op.default === "number"
            ? op.default
            : undefined;
      if (base === undefined || typeof op.value !== "number") continue;
      setDottedPath(draft, op.field, op.op === "add" ? base + op.value : base - op.value);
    }
  }
  return draft;
}

function materializeDynamicAssignActions(config) {
  function convertAction(action) {
    if (
      action &&
      typeof action === "object" &&
      !Array.isArray(action) &&
      Array.isArray(action.$assign_ops)
    ) {
      const ops = action.$assign_ops;
      return assign(({ context, event }) => applyAssignOps(ops, context, event));
    }
    return action;
  }

  function convertActionList(actions) {
    if (!Array.isArray(actions)) return actions;
    return actions.map(convertAction);
  }

  function convertTransition(t) {
    if (t && typeof t === "object" && !Array.isArray(t)) {
      if (t.actions !== undefined) {
        t.actions = convertActionList(t.actions);
      }
    }
    return t;
  }

  function walk(node) {
    if (!node || typeof node !== "object") return;
    if (node.entry !== undefined) node.entry = convertActionList(node.entry);
    if (node.exit !== undefined) node.exit = convertActionList(node.exit);
    if (node.on) {
      for (const [event, transitions] of Object.entries(node.on)) {
        if (Array.isArray(transitions)) {
          node.on[event] = transitions.map(convertTransition);
        } else if (transitions && typeof transitions === "object") {
          convertTransition(transitions);
        }
      }
    }
    if (Array.isArray(node.always)) {
      node.always = node.always.map(convertTransition);
    }
    if (node.states) {
      for (const child of Object.values(node.states)) {
        walk(child);
      }
    }
  }

  walk(config);
}

function collectGuardNames(config) {
  const names = new Set();
  function walk(node) {
    if (node.on) {
      for (const transitions of Object.values(node.on)) {
        const items = Array.isArray(transitions) ? transitions : [transitions];
        for (const t of items) {
          if (typeof t === "object" && t !== null && typeof t.guard === "string") {
            names.add(t.guard);
          }
        }
      }
    }
    if (node.always) {
      const items = Array.isArray(node.always) ? node.always : [node.always];
      for (const t of items) {
        if (typeof t === "object" && t !== null && typeof t.guard === "string") {
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

function collectInvokeSrcNames(config) {
  const names = new Set();
  function walk(node) {
    if (node.invoke) {
      const sources = Array.isArray(node.invoke) ? node.invoke : [node.invoke];
      for (const inv of sources) {
        if (typeof inv.src === "string") {
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

function buildGuardImplementations(guardNames) {
  const guards = {};
  for (const name of guardNames) {
    guards[name] = ({ context, event }) => {
      if (context !== null && typeof context === "object" && name in context) {
        return Boolean(context[name]);
      }
      if (event && typeof event === "object" && name in event) {
        return Boolean(event[name]);
      }
      if (name === "isPositive" && event && typeof event === "object" && "value" in event) {
        return event.value > 0;
      }
      return true;
    };
  }
  return guards;
}

function buildStateNamePathMap(states, prefix = "") {
  const map = {};
  for (const state of states) {
    const path = prefix ? `${prefix}.${state.name}` : state.name;
    map[state.name] = path;
    if (state.children && state.children.length > 0) {
      Object.assign(map, buildStateNamePathMap(state.children, path));
    }
  }
  return map;
}

function irToXstateConfig(ir) {
  const machineId = ir.id || "machine";
  const namePathMap = buildStateNamePathMap(ir.states, machineId);

  function resolveTarget(target) {
    if (!target) return target;
    if (namePathMap[target]) {
      return `#${namePathMap[target]}`;
    }
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

    if (state.entry && state.entry.length > 0) {
      result.entry = state.entry;
    }
    if (state.exit && state.exit.length > 0) {
      result.exit = state.exit;
    }
    if (state.tags && state.tags.length > 0) {
      result.tags = state.tags;
    }

    if (state.transitions && state.transitions.length > 0) {
      const on = {};
      const alwaysTransitions = [];
      for (const t of state.transitions) {
        const isAlways = t.event === "" || t.event === undefined;
        const targets = t.targets && t.targets.length > 0 ? t.targets : (t.target ? [t.target] : []);
        for (const tg of targets) {
          const entry = {};
          if (tg) {
            entry.target = resolveTarget(tg);
          }
          if (t.guard) {
            entry.guard = t.guard.name;
          }
          if (t.actions && t.actions.length > 0) {
            entry.actions = t.actions.map((a) => a.name);
          }
          if (isAlways) {
            alwaysTransitions.push(entry);
          } else {
            if (!on[t.event]) {
              on[t.event] = [];
            }
            on[t.event].push(entry);
          }
        }
      }
      for (const [event, transitions] of Object.entries(on)) {
        if (transitions.length === 1) {
          const t = transitions[0];
          const hasGuard = t.guard !== undefined;
          const hasActions = t.actions !== undefined;
          if (!hasGuard && !hasActions) {
            on[event] = t.target || undefined;
          } else if (!hasActions) {
            on[event] = { target: t.target, guard: t.guard };
          } else {
            on[event] = t;
          }
        }
      }
      result.on = on;
      if (alwaysTransitions.length > 0) {
        result.always = alwaysTransitions;
      }
    }

    if (state.children && state.children.length > 0) {
      result.states = {};
      for (const child of state.children) {
        const childPath = `${path}.${child.name}`;
        result.states[child.name] = convertState(child, childPath);
      }
      if (state.initial) {
        result.initial = state.initial;
      }
    }

    if (state.on_done && state.on_done.length > 0) {
      result.onDone = state.on_done.map((t) => {
        const entry = {};
        if (t.target) entry.target = resolveTarget(t.target);
        if (t.guard) entry.guard = t.guard.name;
        if (t.actions && t.actions.length > 0) entry.actions = t.actions.map((a) => a.name);
        return entry;
      });
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

  const config = {
    id: machineId,
    initial: ir.initial,
  };

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

function stripImplMeta(value) {
  if (Array.isArray(value)) return value.map(stripImplMeta);
  if (value !== null && typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      if (k === "$$impl") continue;
      const stripped = stripImplMeta(v);
      if (stripped !== null && typeof stripped === "object" && !Array.isArray(stripped) && Object.keys(stripped).length === 0) {
        out[k] = k;
      } else {
        out[k] = stripped;
      }
    }
    return out;
  }
  return value;
}

function fixMachineConfigTargets(config) {
  const machineId = config.id || "machine";

  function buildMap(states, prefix) {
    const map = {};
    for (const [name, node] of Object.entries(states || {})) {
      const path = prefix ? `${prefix}.${name}` : name;
      if (!node.id) {
        node.id = path;
      }
      map[name] = path;
      if (node.states) {
        Object.assign(map, buildMap(node.states, path));
      }
    }
    return map;
  }

  const namePathMap = buildMap(config.states, machineId);

  function fixTransitions(state, parentPath) {
    if (state.states) {
      for (const [name, child] of Object.entries(state.states)) {
        const childPath = parentPath ? `${parentPath}.${name}` : `${machineId}.${name}`;
        fixTransitions(child, childPath);
      }
    }

    if (state.on) {
      for (const [event, transitions] of Object.entries(state.on)) {
        if (typeof transitions === "string") {
          const resolved = namePathMap[transitions];
          if (resolved) {
            state.on[event] = `#${resolved}`;
          }
        } else if (Array.isArray(transitions)) {
          for (const t of transitions) {
            if (typeof t.target === "string" && t.target.startsWith("#")) {
              continue;
            }
            if (typeof t.target === "string") {
              const resolved = namePathMap[t.target];
              if (resolved) {
                t.target = `#${resolved}`;
              }
            }
          }
        } else if (transitions && typeof transitions === "object") {
          if (typeof transitions.target === "string" && !transitions.target.startsWith("#")) {
            const resolved = namePathMap[transitions.target];
            if (resolved) {
              transitions.target = `#${resolved}`;
            }
          }
        }
      }
    }
  }

  fixTransitions({ states: config.states }, machineId);
}

const filePath = process.argv[2];
if (!filePath) {
  console.error("Usage: node ts-runner.mjs <test-vector.json>");
  process.exit(1);
}

const raw = readFileSync(filePath, "utf-8");
const parsed = JSON.parse(raw);
const testVectors = Array.isArray(parsed) ? parsed : [parsed];

const results = [];

for (const tv of testVectors) {
  const protocol = tv.protocol || tv;
  const events = protocol.events || tv.events || [];
  const initial = protocol.initial || tv.initial;

  let config;
  if (tv.machine_config) {
    config = JSON.parse(JSON.stringify(tv.machine_config));
    materializeDynamicAssignActions(config);
  } else {
    const ir = tv.machine_ir || tv.machine;
    if (!ir) {
      console.error("No machine_config or machine_ir/machine found in test vector");
      continue;
    }
    config = irToXstateConfig(ir);
  }

  const machineName = tv.machine_name || protocol.machine || config.id || "unknown";

  const guardNames = collectGuardNames(config);
  const guardImpls = buildGuardImplementations(guardNames);

  const invokeSrcNames = collectInvokeSrcNames(config);
  const actorImpls = {};
  for (const name of invokeSrcNames) {
    actorImpls[name] = fromCallback(() => {
      return () => {};
    });
  }

  const machine = createMachine(config, {
    guards: guardImpls,
    actors: actorImpls,
  });

  const actor = createActor(machine);
  actor.start();

  const initialSnapshot = actor.getSnapshot();
  if (initial) {
    const actualValue = stripImplMeta(initialSnapshot.value);
    const actualContext = stripImplMeta(initialSnapshot.context);
    const expectedValue = initial.value;
    const expectedContext = initial.context;
    if (
      JSON.stringify(actualValue) !== JSON.stringify(expectedValue) ||
      JSON.stringify(actualContext) !== JSON.stringify(expectedContext)
    ) {
      console.error(
        `Warning: Initial state mismatch for machine "${machineName}"`
      );
      console.error(
        `  Expected value: ${JSON.stringify(expectedValue)}, got: ${JSON.stringify(actualValue)}`
      );
      console.error(
        `  Expected context: ${JSON.stringify(expectedContext)}, got: ${JSON.stringify(actualContext)}`
      );
    }
  }

  for (const event of events) {
    // `advance_ms` is a harness control field: wait this many ms (real timers,
    // so xstate.js `after` delays elapse) before dispatching the event.
    const { advance_ms, ...dispatch } = event;
    if (advance_ms !== undefined) {
      await new Promise((resolve) => setTimeout(resolve, advance_ms));
    }
    actor.send(dispatch);
    const snapshot = actor.getSnapshot();
    results.push({
      step: results.length,
      event: dispatch,
      snapshot: {
        value: stripImplMeta(snapshot.value),
        context: stripImplMeta(snapshot.context),
      },
      ...(results.length === 0 && tv.skip_compare ? { skip_compare: true } : {}),
    });
  }
}

process.stdout.write(JSON.stringify(results, null, 2) + "\n");
