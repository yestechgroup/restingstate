use xstate_core::{Actions, Guards, Machine};
use xstate_macro::xstate;

xstate!("machines/token-bucket.ts");

#[allow(dead_code)]
struct TokenBucketActions;
impl Actions<Context> for TokenBucketActions {
    fn dispatch(&mut self, name: &str, ctx: &mut Context) {
        if name == "decrementToken" {
            ctx.tokens -= 1;
        }
    }
}

#[allow(dead_code)]
struct TokenBucketGuards;
impl Guards<Context> for TokenBucketGuards {
    fn evaluate(&self, name: &str, ctx: &Context) -> bool {
        if name == "hasTokens" {
            ctx.tokens > 0
        } else {
            true
        }
    }
}

fn main() {
    let machine = MachineImpl;
    let state = machine.initial_state();
    let mut ctx = machine.initial_context();

    println!("Initial state: {:?}", state);
    println!("Initial context: {:?}", ctx);
    println!("Tokens: {}", ctx.tokens);

    let new_state = machine.transition(&State::Ready, &Event::Consume, &mut ctx);
    println!("After CONSUME: {:?}", new_state);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state_is_ready() {
        let machine = MachineImpl;
        assert_eq!(machine.initial_state(), State::Ready);
    }

    #[test]
    fn initial_context_has_ten_tokens() {
        let machine = MachineImpl;
        let ctx = machine.initial_context();
        assert_eq!(ctx.tokens, 10);
    }

    #[test]
    fn consume_with_tokens_stays_ready() {
        let machine = MachineImpl;
        let mut ctx = machine.initial_context();
        let new_state = machine.transition(&State::Ready, &Event::Consume, &mut ctx);
        assert_eq!(new_state, State::Ready);
    }

    #[test]
    fn refill_from_empty_goes_to_ready() {
        let machine = MachineImpl;
        let mut ctx = machine.initial_context();
        let new_state = machine.transition(&State::Empty, &Event::Refill, &mut ctx);
        assert_eq!(new_state, State::Ready);
    }
}
