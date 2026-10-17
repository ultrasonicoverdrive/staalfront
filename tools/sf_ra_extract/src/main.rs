//! `sf_ra_extract <input> --out <dir> [--names <file>] [--dry-run]`; see the
//! library and the README ("Original Red Alert audio").

use std::process::ExitCode;

use sf_audio::ra::crypto::RsaKey;
use sf_ra_extract::{USAGE, parse_args, run};

fn main() -> ExitCode {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("sf_ra_extract: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let mut stdout = std::io::stdout();
    match run(&args, &RsaKey::westwood(), &mut stdout) {
        Ok(summary) => {
            let _ = summary.print(&mut stdout, args.dry_run);
            if summary.sounds.is_empty() {
                eprintln!("sf_ra_extract: no sounds found");
                return ExitCode::FAILURE;
            }
            if !args.dry_run {
                println!(
                    "\nNow start the game with: sf_game --ra-audio \"{}\"",
                    args.out.display()
                );
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("sf_ra_extract: {e}");
            ExitCode::from(2)
        }
    }
}
