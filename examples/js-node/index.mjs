#!/usr/bin/env node
/**
 * js-node-example — Using restingstate from Node.js
 *
 * Prerequisites:
 *   1. Build the native addon: cargo build -p xstate-napi --release
 *   2. The output is at target/release/libxstate_napi.so (Linux) or .dylib/.dll
 *   3. In production, this would be installed via npm install xstate-napi
 *
 * For now, this file demonstrates the usage pattern.
 * To run with actual xstate.js for comparison:
 *   npm install && node index.mjs
 */

import { createMachine, createActor } from 'xstate';

// ============================================
// Part 1: The machine definition (xstate.js format)
// ============================================

const config = {
  id: 'toggle',
  initial: 'inactive',
  context: { count: 0 },
  states: {
    inactive: { on: { TOGGLE: 'active' } },
    active: { on: { TOGGLE: 'inactive' } }
  }
};

// ============================================
// Part 2: How restingstate would use the same machine
// ============================================

// The IR is the JSON-serialized xstate_ir::Machine.
// This is what you'd pass to JsMachine via NAPI-RS.
const machineIr = {
  id: 'toggle',
  initial: 'inactive',
  context: { fields: [{ name: 'count', type_name: 'number', default_value: 0 }] },
  states: [
    {
      name: 'inactive',
      description: null,
      initial: null,
      entry: [],
      exit: [],
      transitions: [{ event: 'TOGGLE', target: 'active', guard: null, actions: [], description: null }],
      children: [],
      type: 'atomic',
      tags: []
    },
    {
      name: 'active',
      description: null,
      initial: null,
      entry: [],
      exit: [],
      transitions: [{ event: 'TOGGLE', target: 'inactive', guard: null, actions: [], description: null }],
      children: [],
      type: 'atomic',
      tags: []
    }
  ]
};

console.log('=== Machine IR (for JsMachine constructor) ===');
console.log(JSON.stringify(machineIr, null, 2));

console.log('\n=== Usage with JsMachine (NAPI-RS) ===');
console.log(`
// With the native addon loaded:
const { JsMachine } = require('xstate-napi');
const machine = new JsMachine(JSON.stringify(machineIr));

console.log(machine.snapshot());
// -> '{"value":"inactive","context":{"count":0}}'

const result = machine.send(JSON.stringify({ type: 'TOGGLE' }));
console.log(result);
// -> '{"value":"active","context":{"count":0}}'

const noMatch = machine.send(JSON.stringify({ type: 'UNKNOWN' }));
console.log(noMatch);
// -> null
`);

// ============================================
// Part 3: Verify the machine works in xstate.js
// ============================================
console.log('=== xstate.js verification ===');
const actor = createActor(createMachine(config));
actor.start();
const snap = actor.getSnapshot();
console.log('Initial:', JSON.stringify({ value: snap.value, context: snap.context }));

actor.send({ type: 'TOGGLE' });
const snap2 = actor.getSnapshot();
console.log('After TOGGLE:', JSON.stringify({ value: snap2.value, context: snap2.context }));
