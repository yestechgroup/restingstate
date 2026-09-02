// Malformed-input robustness matrix — issue #29.
use xstate_parser::{parse, parse_all};

const VALID_MACHINE: &str = r#"
const machine = createMachine({
  id: 'toggle',
  initial: 'inactive',
  context: { count: 0, label: 'a', flags: [1, 2, 3] },
  states: {
    inactive: { on: { toggle: 'active' } },
    active: { on: { toggle: 'inactive' } },
  },
});
"#;

fn deep_array_source(depth: usize) -> String {
    format!(
        "const m = createMachine({{ id: 'm', initial: 'a', context: {{ x: {}{} }}, states: {{ a: {{}} }} }});",
        "[".repeat(depth),
        "]".repeat(depth)
    )
}

fn deep_object_source(depth: usize) -> String {
    format!(
        "const m = createMachine({{ id: 'm', initial: 'a', context: {{ x: {}{} }}, states: {{ a: {{}} }} }});",
        "{\"a\":".repeat(depth),
        "1".to_string() + &"}".repeat(depth)
    )
}

#[test]
fn truncated_sources_return_err_not_panic() {
    for cut in [10, 40, 75, 110, 150, 200, 240, 280] {
        let truncated = &VALID_MACHINE[..cut.min(VALID_MACHINE.len())];
        // Ok or Err are both acceptable; a panic/abort is not (issue #29).
        let _ = parse(truncated);
        let _ = parse_all(truncated);
    }
}

#[test]
fn unbalanced_braces_return_err() {
    let src = "const m = createMachine({ id: 'm', initial: 'a', states: { a: {} });";
    assert!(parse(src).is_err(), "unbalanced braces must be a typed Err");
    let _ = parse_all(src);
}

#[test]
fn missing_comma_returns_err() {
    let src = "const m = createMachine({ id: 'm' initial: 'a', states: { a: {} } });";
    assert!(parse(src).is_err(), "missing comma must be a typed Err");
}

#[test]
fn create_machine_without_args_returns_err() {
    assert!(parse("const m = createMachine();").is_err());
}

#[test]
fn non_object_config_returns_err() {
    assert!(parse("const m = createMachine('hello');").is_err());
    assert!(parse("const m = createMachine(42);").is_err());
    assert!(parse("const m = createMachine(null);").is_err());
    assert!(parse("const m = createMachine([...createMachine({})]);").is_err());
}

#[test]
fn states_with_non_object_values_is_lenient_not_panic() {
    let src = "const m = createMachine({ id: 'm', initial: 'a', states: { a: 'notanobject' } });";
    let machine = parse(src).expect("lenient parse should succeed without panicking");
    assert!(
        machine.states.is_empty(),
        "non-object state values are skipped, got {:?}",
        machine.states
    );
}

#[test]
fn arrow_function_soup_does_not_panic() {
    let src = "const f = () => () => () => 1; const g = (x) => (y) => (z) => x(y(z)); g(f)(f)(f);";
    assert!(parse(src).is_err(), "no createMachine means Err");
    let _ = parse_all(src);
}

#[test]
fn deep_nesting_is_rejected_with_typed_error_not_stack_overflow() {
    for depth in [10_000usize, 5_000] {
        assert!(
            parse(&deep_array_source(depth)).is_err(),
            "array nesting depth {depth} must be rejected, not crash"
        );
        assert!(
            parse(&deep_object_source(depth)).is_err(),
            "object nesting depth {depth} must be rejected, not crash"
        );
        let _ = parse_all(&deep_array_source(depth));
    }
}

#[test]
fn nesting_below_the_cap_still_parses() {
    // Behavior-change guard for the depth cap (issue #29): realistic deep
    // configs must keep parsing.
    let machine = parse(&deep_array_source(300)).expect("depth 300 must still parse");
    assert_eq!(machine.id, "m");
    let machine = parse(&deep_object_source(300)).expect("depth 300 must still parse");
    assert_eq!(machine.id, "m");
}

#[test]
fn huge_string_literal_does_not_panic() {
    let src = format!(
        "const m = createMachine({{ id: '{}', initial: 'a', states: {{ a: {{}} }} }});",
        "x".repeat(1_000_000)
    );
    let machine = parse(&src).expect("a huge id string is valid TS");
    assert_eq!(machine.id.len(), 1_000_000);
}

#[test]
fn unicode_and_zero_width_identifiers_do_not_panic() {
    let ok_src = "const m = createMachine({ id: 'm\u{200b}\u{00e9}\u{1f600}', initial: 'a', states: { a: {} } });";
    let machine = parse(ok_src).expect("zero-width chars inside string literals are fine");
    assert!(machine.id.contains('\u{200b}'));

    let bad_src = "const m\u{200b} = createMachine({ id: 'm', initial: 'a', states: { a: {} } });";
    let _ = parse(bad_src);
    let _ = parse_all(bad_src);
}

#[test]
fn truncated_assign_updater_returns_err() {
    let src = "const m = createMachine({ id: 'm', initial: 'a', states: { a: { on: { E: assign(({context}) => context. } } } });";
    assert!(parse(src).is_err());
}

#[test]
fn parse_all_is_lenient_but_never_panics() {
    // Documented gap: parse_all is per-machine lenient — Ok or Err are both
    // acceptable as long as nothing panics or aborts (issue #29).
    let _ = parse_all("const m = createMachine({ id: 'm', initial: 'a', states: { a: {");
    let _ = parse_all("createMachine(); createMachine('x'); const y = createMachine(1);");
    let _ =
        parse_all("describe('m', () => { it('works', () => { createMachine({ id: 'x' }) }) });");
    let _ = parse_all("");
    let _ = parse_all("(((((((");
    let _ = parse_all("]]]]]]");
}

// Deterministic seeded xorshift64* mutation fuzz — no external deps (#29).
struct Xorshift(u64);

impl Xorshift {
    fn new(seed: u64) -> Self {
        Xorshift(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn seeded_byte_mutation_fuzz_never_panics() {
    let seed: u64 = 0x0000_0029_0000_0001;
    let mut rng = Xorshift::new(seed);
    let original = VALID_MACHINE.as_bytes().to_vec();

    for iteration in 0..500 {
        let mut mutated = original.clone();
        let edits = 1 + rng.below(8);
        for _ in 0..edits {
            if mutated.is_empty() {
                break;
            }
            let pos = rng.below(mutated.len());
            match rng.below(3) {
                0 => {
                    mutated.remove(pos);
                }
                1 => {
                    mutated[pos] = (rng.below(256)) as u8;
                }
                _ => {
                    mutated.insert(pos, (rng.below(256)) as u8);
                }
            }
        }
        let src = String::from_utf8_lossy(&mutated).into_owned();
        // Ok or Err are both fine; a panic or stack overflow is the bug (#29).
        let _ = parse(&src);
        let _ = parse_all(&src);
        let _ = iteration;
    }
    println!("seeded parser fuzz completed with seed {seed:#x}");
}
