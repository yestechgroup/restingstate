import { setup, createMachine } from 'xstate';

const machine = createMachine({
  id: 'setupMachine',
  initial: 'idle',
  context: {},
  states: {
    idle: {
      on: {
        START: 'running'
      }
    },
    running: {
      on: {
        STOP: 'idle'
      }
    }
  }
});
