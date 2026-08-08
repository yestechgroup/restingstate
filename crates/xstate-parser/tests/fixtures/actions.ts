import { createMachine } from 'xstate';

export const formMachine = createMachine({
  id: 'form',
  initial: 'editing',
  context: {},
  states: {
    editing: {
      entry: ['focusField'],
      exit: ['blurField'],
      on: {
        SUBMIT: {
          target: 'submitted',
          actions: ['validate', 'save']
        }
      }
    },
    submitted: {
      entry: ['showSuccess'],
      type: 'final'
    }
  }
});
