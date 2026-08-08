import { createMachine } from 'xstate';

export const authMachine = createMachine({
  id: 'auth',
  initial: 'loggedOut',
  context: {
    isAdmin: false
  },
  states: {
    loggedOut: {
      on: {
        LOGIN: [
          { target: 'admin', guard: 'isAdmin' },
          { target: 'basic' }
        ]
      }
    },
    admin: {
      on: {
        LOGOUT: 'loggedOut'
      }
    },
    basic: {
      on: {
        LOGOUT: 'loggedOut'
      }
    }
  }
});
