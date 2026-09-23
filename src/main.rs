use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;
use docgrep::cli::{Cli, error_message};

fn main() -> ExitCode {
    // Panics are caught per file and reported as file errors; keep stderr clean.
    std::panic::set_hook(Box::new(|_| {}));
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            e.exit()
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "docgrep: エラー: {}", error_message(&e));
            return ExitCode::from(docgrep::EXIT_ERROR);
        }
    };
    ExitCode::from(docgrep::run(&cli))
}
