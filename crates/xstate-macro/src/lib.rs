use proc_macro::TokenStream;
use proc_macro_error2::{abort, proc_macro_error};
use std::path::PathBuf;

/// Generates a Rust state machine from an XState TypeScript file.
///
/// The generated machine is a **flat FSM**: a `(State, Event) -> State` match with
/// typed enums and a context struct. Guards, actions, `after` delays, history,
/// compound/parallel states, and invoke are parsed but **not enforced at runtime**.
/// For full XState semantics, run the machine IR through the `Interpreter`
/// (`xstate-core`) instead — see `docs/runtime-paths.md`.
#[proc_macro]
#[proc_macro_error]
pub fn xstate(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::LitStr);
    let path_str = input.value();

    let path = resolve_path(&path_str);

    let source = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => abort!(
            input.span(),
            "failed to read file '{}': {}",
            path.display(),
            e
        ),
    };

    let machine = match xstate_parser::parse(&source) {
        Ok(m) => m,
        Err(e) => abort!(input.span(), "failed to parse XState machine: {}", e),
    };

    let tokens = match xstate_codegen::generate(&machine) {
        Ok(t) => t,
        Err(e) => abort!(input.span(), "code generation failed: {}", e),
    };

    tokens.into()
}

fn resolve_path(path_str: &str) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        return path;
    }
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR not set (proc macros require cargo)");
    PathBuf::from(manifest_dir).join(path)
}
