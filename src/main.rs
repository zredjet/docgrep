use std::io::Write;
use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;
use docgrep::cli::{Cli, error_message};

fn main() -> ExitCode {
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
