//! docgrep: exact-match search inside Word / Excel files.
//!
//! Pipeline: walk (list files) → extractor (file → text units) → matcher → output.

pub mod cli;
pub mod error;
pub mod matcher;
pub mod model;
pub mod output;
pub mod walk;
pub mod word;

use std::io::{self, Write};

use anstream::{AutoStream, ColorChoice};

use cli::{Cli, ColorWhen};
use error::FileError;
use matcher::Matcher;
use model::{Extracted, Match};
use output::{ContextOpts, FileHits};
use walk::{Item, Kind, Target};

/// Exit codes (SPEC §3.3).
pub const EXIT_MATCH: u8 = 0;
pub const EXIT_NO_MATCH: u8 = 1;
pub const EXIT_ERROR: u8 = 2;

#[derive(Debug, Default, Clone, Copy)]
pub struct Stats {
    /// Files opened for searching (including those that failed).
    pub searched: usize,
    pub hit_files: usize,
    pub hits: usize,
    /// Files reported with a warning.
    pub skipped: usize,
    pub errors: usize,
}

impl Stats {
    pub fn exit_code(&self) -> u8 {
        if self.errors > 0 {
            EXIT_ERROR
        } else if self.hits > 0 {
            EXIT_MATCH
        } else {
            EXIT_NO_MATCH
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Pretty,
    Json,
    FilesWithMatches,
    Count,
}

/// Runs a search and returns the process exit code.
pub fn run(cli: &Cli) -> u8 {
    let mode = if cli.json {
        Mode::Json
    } else if cli.files_with_matches {
        Mode::FilesWithMatches
    } else if cli.count {
        Mode::Count
    } else {
        Mode::Pretty
    };
    let choice = match (mode, cli.color) {
        (Mode::Json, _) | (_, ColorWhen::Never) => ColorChoice::Never,
        (_, ColorWhen::Always) => ColorChoice::Always,
        (_, ColorWhen::Auto) => ColorChoice::Auto,
    };
    let mut out = AutoStream::new(io::stdout(), choice).lock();
    let mut err = AutoStream::new(io::stderr(), choice).lock();

    let matcher = match Matcher::new(&cli.pattern, cli.regex, cli.ignore_case) {
        Ok(m) => m,
        Err(e) => {
            report(&mut err, None, &FileError::Search(e.to_string()));
            return EXIT_ERROR;
        }
    };
    let opts = ContextOpts {
        context: cli.context,
        paragraph: cli.paragraph,
    };
    let paths = if cli.paths.is_empty() {
        vec![std::path::PathBuf::from(".")]
    } else {
        cli.paths.clone()
    };

    let mut stats = Stats::default();
    let mut printed_any = false;
    for item in walk::collect(&paths, cli.max_depth) {
        let target = match item {
            Item::Target(t) => t,
            Item::Problem { display, error } => {
                count_problem(&mut stats, &error);
                report(&mut err, Some(&display), &error);
                continue;
            }
        };
        let (hits, error) = process(&target, &matcher);
        if let Some(e) = &error
            && e.is_warning()
        {
            stats.skipped += 1;
        } else {
            stats.searched += 1;
        }
        if let Some(hits) = &hits
            && !hits.matches.is_empty()
        {
            stats.hit_files += 1;
            stats.hits += hits.matches.len();
            let mut buf = Vec::new();
            if mode == Mode::Pretty && printed_any {
                buf.push(b'\n');
            }
            let rendered = match mode {
                Mode::Pretty => output::pretty::write_file(&mut buf, hits, opts),
                Mode::Json => output::json::write_file(&mut buf, hits, opts),
                Mode::FilesWithMatches => writeln!(buf, "{}", hits.display),
                Mode::Count => writeln!(buf, "{}:{}", hits.display, hits.matches.len()),
            };
            printed_any = true;
            if let Err(e) = rendered
                .and_then(|_| out.write_all(&buf))
                .and_then(|_| out.flush())
            {
                return if e.kind() == io::ErrorKind::BrokenPipe {
                    stats.exit_code()
                } else {
                    EXIT_ERROR
                };
            }
        }
        if let Some(e) = &error {
            if !e.is_warning() {
                stats.errors += 1;
            }
            report(&mut err, Some(&target.display), e);
        }
    }

    if mode == Mode::Pretty {
        let _ = output::pretty::write_summary(&mut err, &stats);
    }
    stats.exit_code()
}

fn count_problem(stats: &mut Stats, error: &FileError) {
    if error.is_warning() {
        stats.skipped += 1;
    } else {
        stats.errors += 1;
    }
}

/// Extracts and searches one file. Returns the hits (possibly partial) and any problem.
fn process(target: &Target, matcher: &Matcher) -> (Option<FileHits>, Option<FileError>) {
    let extracted = match target.kind {
        Kind::Word => word::extract(&target.path),
        Kind::Excel => Err(FileError::ExcelNotYetSupported),
    };
    let Extracted {
        format,
        page_mode,
        units,
        partial_error,
    } = match extracted {
        Ok(x) => x,
        Err(e) => return (None, Some(e)),
    };
    let mut matches = Vec::new();
    let mut error = partial_error;
    for (unit_index, unit) in units.iter().enumerate() {
        match matcher.find(&unit.text) {
            Ok(found) => matches.extend(found.into_iter().map(|(start, end)| Match {
                unit_index,
                start,
                end,
            })),
            Err(e) => {
                error = Some(FileError::Search(e.to_string()));
                break;
            }
        }
    }
    let hits = FileHits {
        display: target.display.clone(),
        format,
        page_mode,
        units,
        matches,
    };
    (Some(hits), error)
}

/// Writes `docgrep: 警告: <path>: <reason>` or the error variant to stderr.
fn report(err: &mut impl Write, display: Option<&str>, error: &FileError) {
    let (style, label) = if error.is_warning() {
        (output::pretty::WARNING, "警告")
    } else {
        (output::pretty::ERROR, "エラー")
    };
    let path = display.map(|d| format!("{d}: ")).unwrap_or_default();
    let _ = writeln!(err, "{style}docgrep: {label}: {path}{error}{style:#}");
}
