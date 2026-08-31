import { setup, createMachine, fromPromise } from 'xstate';

const machine = setup({
  actors: {
    loader: fromPromise(async () => ({ data: 'hello' })),
  },
  guards: {
    isValid: true,
  },
  actions: {
    assignCount: true,
  },
  delays: {
    timeout: 2000,
  },
}).createMachine({
  id: 'fullSetup',
  initial: 'idle',
  context: {},
  states: {
    idle: {
      on: { START: 'loading' },
    },
    loading: {
      after: {
        timeout: 'timeout',
      },
    },
    timeout: {},
  },
});
