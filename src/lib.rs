//! docgrep: exact-match search inside Word / Excel files.
//!
//! Pipeline: walk (list files) → extractor (file → text units) → matcher → output.

pub mod cli;
pub mod error;
pub mod excel;
pub mod matcher;
pub mod model;
pub mod output;
pub mod walk;
pub mod word;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::mpsc;

use anstream::{AutoStream, ColorChoice};
use rayon::prelude::*;

use cli::{Cli, ColorWhen, PartSet};
use error::FileError;
use matcher::Matcher;
use model::{Format, Match, TextUnit};
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

    let items = walk::collect(&paths, cli.max_depth);
    let parts = cli.parts;
    let cancel = AtomicBool::new(false);
    let mut output = Output {
        mode,
        opts,
        out: &mut out,
        err: &mut err,
        stats: Stats::default(),
        printed_any: false,
    };

    // Files are processed in parallel; results are printed in argument order.
    let finished = std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel::<(usize, Outcome)>();
        let matcher = &matcher;
        let cancel = &cancel;
        scope.spawn(move || {
            let work = || {
                items
                    .into_par_iter()
                    .enumerate()
                    .for_each_with(tx, |tx, (i, item)| {
                        if cancel.load(AtomicOrdering::Relaxed) {
                            return;
                        }
                        // A send error means output has stopped; nothing to do.
                        let _ = tx.send((i, process_item(item, matcher, &parts)));
                    });
            };
            match thread_pool(cli.threads) {
                Some(pool) => pool.install(work),
                None => work(),
            }
        });

        let mut pending: BTreeMap<usize, Outcome> = BTreeMap::new();
        let mut next = 0;
        for (i, outcome) in rx {
            pending.insert(i, outcome);
            while let Some(outcome) = pending.remove(&next) {
                next += 1;
                if let Err(code) = output.emit(outcome) {
                    cancel.store(true, AtomicOrdering::Relaxed);
                    return Err(code);
                }
            }
        }
        Ok(())
    });
    if let Err(code) = finished {
        return code;
    }

    let stats = output.stats;
    if mode == Mode::Pretty {
        let _ = output::pretty::write_summary(&mut err, &stats);
    }
    stats.exit_code()
}

/// A rayon pool with `threads` workers; `None` (or 0) uses the global pool.
fn thread_pool(threads: Option<usize>) -> Option<rayon::ThreadPool> {
    let n = threads.filter(|&n| n > 0)?;
    rayon::ThreadPoolBuilder::new().num_threads(n).build().ok()
}

/// Result of one command line item, ready to be printed.
enum Outcome {
    Problem {
        display: String,
        error: FileError,
    },
    File {
        display: String,
        hits: Option<FileHits>,
        error: Option<FileError>,
    },
}

fn process_item(item: Item, matcher: &Matcher, parts: &PartSet) -> Outcome {
    match item {
        Item::Problem { display, error } => Outcome::Problem { display, error },
        Item::Target(target) => {
            // Never let a panic in a parser take down the whole run.
            let result =
                std::panic::catch_unwind(AssertUnwindSafe(|| process(&target, matcher, parts)));
            let (hits, error) = result.unwrap_or((None, Some(FileError::Internal)));
            Outcome::File {
                display: target.display,
                hits,
                error,
            }
        }
    }
}

/// Prints outcomes in order and keeps the statistics.
struct Output<'a, W: Write, E: Write> {
    mode: Mode,
    opts: ContextOpts,
    out: &'a mut W,
    err: &'a mut E,
    stats: Stats,
    printed_any: bool,
}

impl<W: Write, E: Write> Output<'_, W, E> {
    /// Prints one outcome. `Err` carries the exit code when stdout is gone.
    fn emit(&mut self, outcome: Outcome) -> Result<(), u8> {
        let (display, hits, error) = match outcome {
            Outcome::Problem { display, error } => {
                count_problem(&mut self.stats, &error);
                report(self.err, Some(&display), &error);
                return Ok(());
            }
            Outcome::File {
                display,
                hits,
                error,
            } => (display, hits, error),
        };
        if error.as_ref().is_some_and(FileError::is_warning) {
            self.stats.skipped += 1;
        } else {
            self.stats.searched += 1;
        }
        if let Some(hits) = &hits
            && !hits.matches.is_empty()
        {
            self.stats.hit_files += 1;
            self.stats.hits += hits.matches.len();
            let mut buf = Vec::new();
            if self.mode == Mode::Pretty && self.printed_any {
                buf.push(b'\n');
            }
            let rendered = match self.mode {
                Mode::Pretty => output::pretty::write_file(&mut buf, hits, self.opts),
                Mode::Json => output::json::write_file(&mut buf, hits, self.opts),
                Mode::FilesWithMatches => writeln!(buf, "{}", hits.display),
                Mode::Count => writeln!(buf, "{}:{}", hits.display, hits.matches.len()),
            };
            self.printed_any = true;
            if let Err(e) = rendered
                .and_then(|_| self.out.write_all(&buf))
                .and_then(|_| self.out.flush())
            {
                return Err(if e.kind() == io::ErrorKind::BrokenPipe {
                    self.stats.exit_code()
                } else {
                    EXIT_ERROR
                });
            }
        }
        if let Some(e) = &error {
            if !e.is_warning() {
                self.stats.errors += 1;
            }
            report(self.err, Some(&display), e);
        }
        Ok(())
    }
}

fn count_problem(stats: &mut Stats, error: &FileError) {
    if error.is_warning() {
        stats.skipped += 1;
    } else {
        stats.errors += 1;
    }
}

/// Search results of one file. Only units with matches are kept, so large files
/// (Excel sheets in particular) do not stay in memory.
struct Collector<'m> {
    matcher: &'m Matcher,
    units: Vec<TextUnit>,
    matches: Vec<Match>,
    error: Option<FileError>,
}

impl<'m> Collector<'m> {
    fn new(matcher: &'m Matcher) -> Self {
        Collector {
            matcher,
            units: Vec::new(),
            matches: Vec::new(),
            error: None,
        }
    }

    /// Searches one unit. Returns false once searching has failed.
    fn add(&mut self, unit: TextUnit) -> bool {
        if self.error.is_some() {
            return false;
        }
        match self.matcher.find(&unit.text) {
            Ok(found) if found.is_empty() => true,
            Ok(found) => {
                let unit_index = self.units.len();
                self.units.push(unit);
                self.matches
                    .extend(found.into_iter().map(|(start, end)| Match {
                        unit_index,
                        start,
                        end,
                    }));
                true
            }
            Err(e) => {
                self.error = Some(FileError::Search(e.to_string()));
                false
            }
        }
    }
}

/// Extracts and searches one file. Returns the hits (possibly partial) and any problem.
fn process(
    target: &Target,
    matcher: &Matcher,
    parts: &PartSet,
) -> (Option<FileHits>, Option<FileError>) {
    let mut collector = Collector::new(matcher);
    let extracted = match target.kind {
        Kind::Word => word::extract(&target.path).map(|x| {
            for unit in x.units {
                if !parts.allows(&unit.location.part) {
                    continue;
                }
                if !collector.add(unit) {
                    break;
                }
            }
            (x.format, x.page_mode, x.partial_error)
        }),
        Kind::Excel => excel::extract_with(&target.path, |unit| collector.add(unit))
            .map(|partial| (Format::Excel, None, partial)),
    };
    let (format, page_mode, partial_error) = match extracted {
        Ok(x) => x,
        Err(e) => return (None, Some(e)),
    };
    // A search failure is reported in preference to a partial extraction error.
    let error = collector.error.or(partial_error);
    let hits = FileHits {
        display: target.display.clone(),
        format,
        page_mode,
        units: collector.units,
        matches: collector.matches,
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
