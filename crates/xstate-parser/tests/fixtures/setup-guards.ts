import { setup, createMachine } from 'xstate';

const machine = setup({
  guards: {
    isValid: true,
    isAdult: true,
  },
}).createMachine({
  id: 'withGuards',
  initial: 'idle',
  context: {},
  states: {
    idle: {
      on: { SUBMIT: { target: 'done', guard: 'isValid' } },
    },
    done: {},
  },
});
