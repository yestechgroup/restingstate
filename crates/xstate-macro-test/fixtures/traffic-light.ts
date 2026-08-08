import { createMachine } from 'xstate';

export const trafficMachine = createMachine({
  id: 'trafficLight',
  initial: 'red',
  context: {},
  states: {
    green: {
      on: {
        TIMER: 'yellow'
      }
    },
    yellow: {
      on: {
        TIMER: 'red'
      }
    },
    red: {
      type: 'compound',
      initial: 'walk',
      states: {
        walk: {
          on: {
            RESET: 'stop'
          }
        },
        stop: {
          type: 'final'
        }
      },
      on: {
        TIMER: 'green'
      }
    }
  }
});
