//! Expands command line paths into the list of files to search.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::FileError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Word,
    Excel,
}

#[derive(Debug)]
pub struct Target {
    pub path: PathBuf,
    /// Path as shown to the user.
    pub display: String,
    pub kind: Kind,
}

#[derive(Debug)]
pub enum Item {
    Target(Target),
    Problem { display: String, error: FileError },
}

/// Classifies a file by extension (case-insensitive).
pub fn kind_of(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "docm" | "dotx" | "dotm" => Some(Kind::Word),
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xlsb" | "xls" | "ods" => Some(Kind::Excel),
        _ => None,
    }
}

/// Files that are always skipped silently: Office lock files (`~$name.docx`) and
/// macOS AppleDouble metadata files (`._name.docx`).
fn is_ignored_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("~$") || n.starts_with("._"))
}

/// Expands arguments in order. Directories are walked recursively in name order.
pub fn collect(paths: &[PathBuf], max_depth: Option<usize>) -> Vec<Item> {
    let mut items = Vec::new();
    for path in paths {
        let display = path.display().to_string();
        if path.is_dir() {
            walk_dir(path, max_depth, &mut items);
        } else if !path.exists() {
            items.push(Item::Problem {
                display,
                error: FileError::NotFound,
            });
        } else if is_ignored_file(path) {
            continue;
        } else {
            match kind_of(path) {
                Some(kind) => items.push(Item::Target(Target {
                    path: path.clone(),
                    display,
                    kind,
                })),
                None => items.push(Item::Problem {
                    display,
                    error: FileError::Unsupported,
                }),
            }
        }
    }
    items
}

fn walk_dir(root: &Path, max_depth: Option<usize>, items: &mut Vec<Item>) {
    let mut walker = WalkDir::new(root).follow_links(false).sort_by_file_name();
    if let Some(depth) = max_depth {
        walker = walker.max_depth(depth);
    }
    for entry in walker {
        match entry {
            Ok(entry) => {
                if !entry.file_type().is_file() || is_ignored_file(entry.path()) {
                    continue;
                }
                let Some(kind) = kind_of(entry.path()) else {
                    continue;
                };
                items.push(Item::Target(Target {
                    path: entry.path().to_path_buf(),
                    display: display_path(root, entry.path()),
                    kind,
                }));
            }
            Err(err) => {
                let display = err
                    .path()
                    .map(|p| display_path(root, p))
                    .unwrap_or_else(|| root.display().to_string());
                let error = match err.into_io_error() {
                    Some(io) => FileError::Io(io),
                    None => FileError::Io(std::io::Error::other("ディレクトリを探索できません")),
                };
                items.push(Item::Problem { display, error });
            }
        }
    }
}

/// Paths found under `.` are shown without the leading `./`.
fn display_path(root: &Path, path: &Path) -> String {
    if root == Path::new(".")
        && let Ok(rel) = path.strip_prefix(".")
    {
        return rel.display().to_string();
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension() {
        assert_eq!(kind_of(Path::new("a.DOCX")), Some(Kind::Word));
        assert_eq!(kind_of(Path::new("a.dotm")), Some(Kind::Word));
        assert_eq!(kind_of(Path::new("a.xls")), Some(Kind::Excel));
        assert_eq!(kind_of(Path::new("a.doc")), None);
        assert_eq!(kind_of(Path::new("docx")), None);
    }

    #[test]
    fn ignored_files() {
        assert!(is_ignored_file(Path::new("dir/._仕様書.docx")));
        assert!(is_ignored_file(Path::new("dir/~$仕様書.docx")));
        assert!(!is_ignored_file(Path::new("dir/仕様書.docx")));
    }

    #[test]
    fn dot_root_is_stripped() {
        assert_eq!(
            display_path(Path::new("."), Path::new("./sub/a.docx")),
            Path::new("sub/a.docx").display().to_string()
        );
        assert_eq!(
            display_path(Path::new("docs"), Path::new("docs/a.docx")),
            Path::new("docs/a.docx").display().to_string()
        );
    }
}
