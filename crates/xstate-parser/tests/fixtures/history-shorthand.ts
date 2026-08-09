import { createMachine } from 'xstate';

export const machine = createMachine({
  id: 'historyShorthand',
  initial: 'on',
  context: {},
  states: {
    on: {
      initial: 'first',
      states: {
        first: { on: { SWITCH: 'second' } },
        second: {},
        hist: {
          history: true
        }
      },
      on: { POWER: 'off' }
    },
    off: {
      on: { POWER: 'on.hist' }
    }
  }
});
