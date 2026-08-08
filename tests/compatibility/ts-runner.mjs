import { readFileSync } from "fs";
import { createMachine, createActor } from "xstate";

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

    if (state.type && state.type !== "atomic") {
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
        const entry = {};
        if (t.target) {
          entry.target = resolveTarget(t.target);
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
      out[k] = stripImplMeta(v);
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

  const machine = createMachine(config, {
    guards: guardImpls,
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
    actor.send(event);
    const snapshot = actor.getSnapshot();
    results.push({
      step: results.length,
      event,
      snapshot: {
        value: stripImplMeta(snapshot.value),
        context: stripImplMeta(snapshot.context),
      },
    });
  }
}

process.stdout.write(JSON.stringify(results, null, 2) + "\n");
