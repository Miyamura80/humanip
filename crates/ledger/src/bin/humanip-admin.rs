//! `humanip-admin --db PATH <command>`: operate the humanip ledger.

use humanip_ledger::cli::{run, CliError, USAGE};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let mut out = std::io::stdout().lock();
    match run(&args, &mut out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(CliError::Usage(m)) => {
            eprintln!("error: {m}\n\n{USAGE}");
            ExitCode::from(2)
        }
        Err(CliError::Failed(m)) => {
            eprintln!("error: {m}");
            ExitCode::FAILURE
        }
    }
}
