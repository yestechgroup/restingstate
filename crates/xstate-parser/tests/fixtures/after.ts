import { createMachine } from 'xstate';

export const machine = createMachine({
  id: 'afterTest',
  initial: 'green',
  context: {},
  states: {
    green: {
      after: {
        1000: 'yellow'
      }
    },
    yellow: {
      after: {
        1000: 'red'
      }
    },
    red: {}
  }
});
