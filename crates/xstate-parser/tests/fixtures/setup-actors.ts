import { setup, createMachine, fromPromise, fromCallback } from 'xstate';

const machine = setup({
  actors: {
    loader: fromPromise(async () => ({ data: 'hello' })),
    timer: fromCallback(() => () => {}),
  },
}).createMachine({
  id: 'withActors',
  initial: 'idle',
  context: {},
  states: {
    idle: {
      on: { START: 'running' },
    },
    running: {},
  },
});
