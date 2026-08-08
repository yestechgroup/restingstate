import { createMachine } from 'xstate';

export const machine = createMachine({
  id: 'tokenBucket',
  initial: 'ready',
  context: { tokens: 10 },
  states: {
    ready: {
      on: {
        CONSUME: [
          { target: 'ready', guard: 'hasTokens' },
          { target: 'empty' }
        ]
      }
    },
    empty: {
      on: {
        REFILL: { target: 'ready' }
      }
    }
  }
});
