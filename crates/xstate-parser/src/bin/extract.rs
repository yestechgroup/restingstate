use std::fs;
use std::io::Read;
use std::process::ExitCode;

/// Prints every machine found in the given TypeScript file as a JSON array of
/// `xstate_ir::Machine` objects, in source order. Exits with code 1 (and a
/// JSON `{"error": ...}` payload on stdout) if the file cannot be parsed.
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: extract <file.ts>");
        return ExitCode::FAILURE;
    }

    let path = &args[1];
    let mut source = String::new();
    if let Err(e) = fs::File::open(path).and_then(|mut f| f.read_to_string(&mut source)) {
        eprintln!("Error reading {}: {}", path, e);
        return ExitCode::FAILURE;
    }

    match xstate_parser::parse_all(&source) {
        Ok(machines) => match serde_json::to_string_pretty(&machines) {
            Ok(json) => {
                println!("{}", json);
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("Error serializing machines: {}", e);
                ExitCode::FAILURE
            }
        },
        Err(e) => {
            println!("{}", serde_json::json!({ "error": e.to_string() }));
            ExitCode::FAILURE
        }
    }
}
