//! Runs the simulation without a screen: tests, balance, AI-vs-AI.
//!
//! See the `sf_headless` library documentation for the scenario format, the
//! log and the exit codes; `sf_headless --help` for the options.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(arg) => {
                eprintln!("sf_headless: argument {arg:?} is not valid UTF-8");
                return ExitCode::from(sf_headless::cli::EXIT_INVALID);
            }
        }
    }
    let code = sf_headless::cli::run_cli(
        args,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
