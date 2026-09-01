// JS companion to docs/embedding.md (issue #33).
// Build the wasm pkg first: see README.md. Run with: node demo.mjs
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const {
  init,
  validateMachine,
  JsMachine,
  JsActorSystem,
  registerGuard,
  registerAction,
  clearRegistry,
} = require('./pkg/xstate_wasm.js');

// Machine-IR JSON mirrors examples/embed (same wallet machine, lowered to IR
// instead of parsed from TS). See schema/machine-ir.schema.json.
const walletIr = {
  id: 'wallet',
  initial: 'open',
  context: {
    fields: [
      { name: 'balance', type_name: 'number', default_value: 10 },
      { name: 'spent', type_name: 'number', default_value: 0 },
    ],
  },
  states: [
    {
      name: 'open',
      type: 'atomic',
      transitions: [
        {
          event: 'ADD',
          target: 'open',
          actions: [
            { name: 'assign', args: [{ $assign_ops: [{ op: 'add', field: 'balance', value: 20 }] }] },
          ],
        },
        {
          event: 'SPEND',
          target: 'locked',
          guard: { name: 'canSpend' },
          actions: [{ name: 'recordSpend' }],
        },
        { event: 'SPEND', target: 'denied' },
      ],
    },
    { name: 'locked', type: 'atomic', transitions: [{ event: 'RETRY', target: 'open' }] },
    { name: 'denied', type: 'atomic', transitions: [{ event: 'RETRY', target: 'open' }] },
  ],
};

const jobIr = {
  id: 'job',
  initial: 'working',
  context: { fields: [] },
  states: [
    {
      name: 'working',
      type: 'atomic',
      transitions: [{ event: 'FINISH', target: 'done' }],
    },
    { name: 'done', type: 'final' },
  ],
};

await init();

// 1. Validate gate before spawn (same checks as validate_machine_json in Rust).
assert.equal(validateMachine(JSON.stringify(walletIr)), 'ok');
assert.throws(
  () =>
    validateMachine(
      JSON.stringify({
        id: 'broken',
        initial: 'nope',
        context: { fields: [] },
        states: [{ name: 'only', type: 'atomic', transitions: [] }],
      }),
    ),
  /initial state/,
);

// 2. JS guards get REAL JS objects (context and event), not JSON strings.
clearRegistry();
const guardCalls = [];
registerGuard('canSpend', (ctx, event) => {
  assert.equal(typeof ctx, 'object');
  assert.equal(typeof event, 'object');
  guardCalls.push({ balance: ctx.balance, cost: event.cost });
  return ctx.balance >= event.cost;
});
const spends = [];
registerAction('recordSpend', (args, event) => {
  assert(Array.isArray(args));
  spends.push(event.cost);
});

// 3. JsMachine: send events, assert snapshot transitions.
const machine = new JsMachine(JSON.stringify(walletIr));
assert.deepEqual(JSON.parse(machine.snapshot()), {
  value: 'open',
  context: { balance: 10, spent: 0 },
});

machine.send(JSON.stringify({ type: 'SPEND', cost: 25 }));
assert.equal(JSON.parse(machine.snapshot()).value, 'denied', 'guard false -> fall-through');
assert.equal(guardCalls.at(-1).balance, 10);

machine.send(JSON.stringify({ type: 'RETRY' }));
machine.send(JSON.stringify({ type: 'ADD' }));
assert.deepEqual(JSON.parse(machine.snapshot()).context, { balance: 30, spent: 0 });

machine.send(JSON.stringify({ type: 'SPEND', cost: 25 }));
assert.deepEqual(JSON.parse(machine.snapshot()), {
  value: 'locked',
  context: { balance: 30, spent: 0 },
});
assert.deepEqual(spends, [25], 'JS action received the real event object');

// 4. Persistence round-trip (versioned envelope, version 1).
const persisted = JSON.parse(machine.persistedSnapshot());
assert.equal(persisted.version, 1);
assert.deepEqual(JSON.parse(machine.snapshot()), {
  value: persisted.value,
  context: persisted.context,
});
const restored = JsMachine.restore(JSON.stringify(walletIr), JSON.stringify(persisted));
assert.deepEqual(JSON.parse(restored.snapshot()), JSON.parse(machine.snapshot()));
restored.send(JSON.stringify({ type: 'RETRY' }));
assert.equal(JSON.parse(restored.snapshot()).value, 'open', 'restored machine keeps running');

// 5. JsActorSystem: spawn from a persisted snapshot + status "done" on final.
const system = new JsActorSystem();
system.spawnFromState('wallet2', JSON.stringify(walletIr), JSON.stringify(persisted));
assert.equal(JSON.parse(system.snapshot('wallet2')).context.balance, 30);
system.sendTo('wallet2', JSON.stringify({ type: 'RETRY' }));
assert.equal(JSON.parse(system.snapshot('wallet2')).value, 'open');

system.spawn('job1', JSON.stringify(jobIr));
assert.equal(system.status('job1'), 'running');
system.sendTo('job1', JSON.stringify({ type: 'FINISH' }));
assert.equal(system.status('job1'), 'done', 'final state reports the pinned "done" status');

console.log('js-embed demo passed:');
console.log('  wallet:', JSON.stringify(JSON.parse(machine.snapshot())));
console.log('  persisted version:', persisted.version);
console.log('  actor wallet2:', JSON.stringify(JSON.parse(system.snapshot('wallet2'))));
console.log('  job1 status:', system.status('job1'));
