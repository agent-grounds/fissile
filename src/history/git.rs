//! Bounded Git plumbing for historical audit snapshots
//! (§FS-004-check-audit.2.1).

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{HistoryError, Revision};

static SCRATCH: AtomicUsize = AtomicUsize::new(0);

pub(crate) struct Repository<'a> {
    root: &'a Path,
    range: &'a str,
}

pub(crate) struct Materialized {
    pub root: PathBuf,
    pub blobs: BTreeMap<String, String>,
}

impl Drop for Materialized {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Rename {
    pub old: String,
    pub new: String,
    pub kind: RenameKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RenameKind {
    ExactFile,
    Directory,
}

impl Repository<'_> {
    pub fn open<'a>(root: &'a Path, range: &'a str) -> Result<Repository<'a>, HistoryError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map_err(|_| HistoryError::new(range, "not a Git work tree"))?;
        if !output.status.success() || output.stdout != b"true\n" {
            return Err(HistoryError::new(range, "not a Git work tree"));
        }
        Ok(Repository { root, range })
    }

    pub fn resolve(&self, expression: &str) -> Result<String, HistoryError> {
        // `--end-of-options` makes a caller-controlled revision data, never an
        // option (§FS-004-check-audit.2.1).
        let requested = format!("{expression}^{{commit}}");
        let output = self.run(&["rev-parse", "--verify", "--end-of-options", &requested])?;
        if !output.status.success() {
            return Err(HistoryError::new(
                self.range,
                format!("revision `{expression}` does not resolve to a commit"),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    pub fn require_ancestor(
        &self,
        from_expr: &str,
        from: &str,
        to: &str,
    ) -> Result<(), HistoryError> {
        let output = self.run(&["merge-base", "--is-ancestor", "--", from, to])?;
        if !output.status.success() {
            return Err(HistoryError::new(
                self.range,
                format!(
                    "`{from_expr}` is not an ancestor of `{}`",
                    self.to_expression()
                ),
            ));
        }
        Ok(())
    }

    fn to_expression(&self) -> &str {
        self.range.split_once("..").map_or("", |(_, to)| to)
    }

    pub fn revisions(&self, to: &str) -> Result<Vec<Revision>, HistoryError> {
        let output = self.run(&[
            "log",
            "--topo-order",
            "--reverse",
            "--format=%H%x00%ct%x00%P",
            to,
            "--",
        ])?;
        if !output.status.success() {
            return Err(HistoryError::new(self.range, "Git history is unavailable"));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        text.lines()
            .map(|line| {
                let mut fields = line.split('\0');
                let sha = fields.next().unwrap_or("");
                let seconds = fields.next().ok_or_else(|| {
                    HistoryError::new(self.range, "Git returned malformed commit metadata")
                })?;
                let parents = fields.next().ok_or_else(|| {
                    HistoryError::new(self.range, "Git returned malformed commit metadata")
                })?;
                if sha.is_empty() || fields.next().is_some() {
                    return Err(HistoryError::new(
                        self.range,
                        "Git returned malformed commit metadata",
                    ));
                }
                let timestamp = seconds.parse::<i64>().map_err(|_| {
                    HistoryError::new(self.range, "Git returned malformed commit metadata")
                })?;
                Ok(Revision {
                    sha: sha.to_owned(),
                    timestamp,
                    parents: parents.split_whitespace().map(str::to_owned).collect(),
                })
            })
            .collect()
    }

    pub fn shallow_boundaries(&self) -> Result<BTreeSet<String>, HistoryError> {
        let output = self.run(&["rev-parse", "--is-shallow-repository"])?;
        if !output.status.success() || output.stdout != b"true\n" {
            return Ok(BTreeSet::new());
        }
        let output = self.run(&["rev-parse", "--git-path", "shallow"])?;
        if !output.status.success() {
            return Err(HistoryError::new(
                self.range,
                "shallow boundary metadata is unavailable",
            ));
        }
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let path = Path::new(&raw);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let text = fs::read_to_string(path).map_err(|_| {
            HistoryError::new(self.range, "shallow boundary metadata is unavailable")
        })?;
        Ok(text.lines().map(str::to_owned).collect())
    }

    pub fn materialize(&self, sha: &str) -> Result<Materialized, HistoryError> {
        let listing = self.run(&["ls-tree", "-r", "-z", sha])?;
        if !listing.status.success() {
            return Err(self.at(sha, "committed tree is unavailable"));
        }
        let number = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("fissile-history-{}-{number}", std::process::id()));
        fs::create_dir_all(&root).map_err(|error| self.io_at(sha, error))?;
        let mut blobs = BTreeMap::new();
        for record in listing
            .stdout
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
        {
            let record = String::from_utf8_lossy(record);
            let (metadata, path) = record
                .split_once('\t')
                .ok_or_else(|| self.at(sha, "Git returned malformed tree metadata"))?;
            let mut fields = metadata.split_whitespace();
            let mode = fields.next().unwrap_or("");
            let _kind = fields.next();
            let blob = fields
                .next()
                .ok_or_else(|| self.at(sha, "Git returned malformed tree metadata"))?;
            blobs.insert(path.to_owned(), blob.to_owned());
            let target = safe_target(&root, path)
                .ok_or_else(|| self.at(sha, format!("unsafe path in committed tree: {path}")))?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| self.io_at(sha, error))?;
            }
            if mode == "120000" {
                continue;
            }
            let content = self.run(&["cat-file", "blob", blob])?;
            if !content.status.success() {
                return Err(self.at(sha, format!("blob for {path} is unavailable")));
            }
            fs::write(&target, content.stdout).map_err(|error| self.io_at(sha, error))?;
            make_executable(&target, mode).map_err(|error| self.io_at(sha, error))?;
        }
        Ok(Materialized { root, blobs })
    }

    pub fn renames(&self, old: &str, new: &str) -> Result<Vec<Rename>, HistoryError> {
        let output = self.run(&[
            "diff-tree",
            "-r",
            "-M",
            "--name-status",
            "-z",
            old,
            new,
            "--",
        ])?;
        if !output.status.success() {
            return Err(self.at(new, "rename evidence is unavailable"));
        }
        let fields: Vec<String> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|field| !field.is_empty())
            .map(|field| String::from_utf8_lossy(field).into_owned())
            .collect();
        let mut result = Vec::new();
        let mut index = 0;
        while index < fields.len() {
            let status = &fields[index];
            index += 1;
            if status.starts_with('R') {
                if index + 1 >= fields.len() {
                    return Err(self.at(new, "Git returned malformed rename evidence"));
                }
                let old_path = fields[index].clone();
                let new_path = fields[index + 1].clone();
                index += 2;
                result.push(Rename {
                    old: old_path,
                    new: new_path,
                    kind: RenameKind::ExactFile,
                });
            } else {
                index += 1;
            }
        }
        Ok(result)
    }

    fn run(&self, args: &[&str]) -> Result<std::process::Output, HistoryError> {
        Command::new("git")
            .arg("-C")
            .arg(self.root)
            .args(args)
            .output()
            .map_err(|error| HistoryError::new(self.range, format!("Git is unavailable: {error}")))
    }

    fn at(&self, revision: &str, cause: impl Into<String>) -> HistoryError {
        HistoryError::new(self.range, format!("revision {revision}: {}", cause.into()))
    }

    pub fn error_at(&self, revision: &str, cause: impl Into<String>) -> HistoryError {
        self.at(revision, cause)
    }

    fn io_at(&self, revision: &str, error: io::Error) -> HistoryError {
        self.at(
            revision,
            format!("snapshot materialization failed: {error}"),
        )
    }
}

fn safe_target(root: &Path, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path);
    if relative
        .components()
        .all(|component| matches!(component, std::path::Component::Normal(_)))
    {
        Some(root.join(relative))
    } else {
        None
    }
}

#[cfg(unix)]
fn make_executable(path: &Path, mode: &str) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    if mode == "100755" {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path, _mode: &str) -> io::Result<()> {
    Ok(())
}
