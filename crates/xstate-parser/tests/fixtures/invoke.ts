import { createMachine } from 'xstate';

export const machine = createMachine({
  id: 'invokeTest',
  initial: 'loading',
  context: {},
  states: {
    loading: {
      invoke: {
        src: 'fetchData',
        id: 'loader',
        onDone: 'loaded'
      }
    },
    loaded: {}
  }
});
