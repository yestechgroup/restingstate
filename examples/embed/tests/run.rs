// Issue #33: the demo asserts, so it doubles as a workspace test.
#[test]
fn embedding_walkthrough() {
    embed::run().expect("embedding walkthrough should pass");
}
