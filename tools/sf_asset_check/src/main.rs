//! Checks delivered .glb models against the art brief.
//!
//! See the `sf_asset_check` library documentation for the rules;
//! `sf_asset_check --help` for the options.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(arg) => {
                eprintln!("sf_asset_check: argument {arg:?} is not valid UTF-8");
                return ExitCode::from(sf_asset_check::cli::EXIT_USAGE);
            }
        }
    }
    let code = sf_asset_check::cli::run_cli(
        &args,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    ExitCode::from(code)
}
