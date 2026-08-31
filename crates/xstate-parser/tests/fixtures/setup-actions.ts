import { setup, createMachine } from 'xstate';

const machine = setup({
  actions: {
    assignCount: true,
    logEvent: true,
  },
}).createMachine({
  id: 'withActions',
  initial: 'idle',
  context: {},
  states: {
    idle: {
      on: { DO: { target: 'done', actions: 'assignCount' } },
    },
    done: {},
  },
});
