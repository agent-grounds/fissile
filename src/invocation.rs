//! The invocation context (§FS-001-config.8.1): where a command was started,
//! relative to the root of the Git work tree it governs.
//!
//! The binary resolves it once, before any command runs, from one `git
//! rev-parse`, and hands every command the root it yields as the `root` its
//! options already carry. That root is spelled from the invocation directory:
//! `.` at the top, `..` one level down. So what the config names resolves from
//! the root by joining it, and a path that locates a config document opens
//! from where the caller stands.
//!
//! The library reads that spelling back, and so does every `root` a library
//! caller passes on a command's options. A root made only of `..` steps, `..`
//! or `../..`, says the caller stands that many levels below it, in the
//! process's working directory: a path handed in with it is relative to that
//! directory and is rebased onto the root (§FS-007-measure.1), so `x.py` with
//! root `..` from `sub/` is `sub/x.py`. A config in that directory is the one
//! passed over (§FS-001-config.8.4), and a path inside a command a run offers
//! is spelled from there (§FS-001-config.8.1). Any other root, `.` or an
//! absolute path included, puts the caller at the root, and the paths handed in
//! with it stay relative to it. To keep root-relative paths with a root above
//! the working directory, pass that root absolute.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::config::{CONFIG_HOME, DEPRECATED_CONFIG_HOME};

/// What one `git rev-parse` says about a directory (§FS-001-config.8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Probe {
    /// The work tree's root, absolute, as Git resolves it (symlinks followed).
    pub toplevel: PathBuf,
    /// The way up to the root: empty at the root, `../` one level below it.
    pub cdup: String,
    /// The way down from the root: empty at the root, `sub/` one level below it.
    pub prefix: String,
}

/// Ask Git where `dir` sits, or `None` outside a work tree or when Git cannot
/// answer. The one probe: the invocation context, `init`'s warning
/// (§FS-002-init.1) and audit's empty-scan note (§FS-004-check-audit.2.2) all
/// read it.
pub(crate) fn probe(dir: &Path) -> Option<Probe> {
    read_probe(&mut probe_command(dir))
}

fn probe_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args([
        "rev-parse",
        "--show-toplevel",
        "--show-cdup",
        "--show-prefix",
    ]);
    command
}

fn read_probe(command: &mut Command) -> Option<Probe> {
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_probe(&String::from_utf8_lossy(&output.stdout))
}

/// One line per question, in the order asked; at the root the last two are empty.
fn parse_probe(stdout: &str) -> Option<Probe> {
    let mut lines = stdout.lines();
    let toplevel = lines.next().filter(|top| !top.is_empty())?;
    Some(Probe {
        toplevel: PathBuf::from(toplevel),
        cdup: lines.next().unwrap_or_default().to_owned(),
        prefix: lines.next().unwrap_or_default().to_owned(),
    })
}

/// Where this process was started, relative to the root it governs
/// (§FS-001-config.8.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    root: PathBuf,
    prefix: String,
    /// The work tree's root, absolute; held only below it, where an explicit
    /// `--config` is resolved against the invocation directory.
    toplevel: Option<PathBuf>,
}

impl Invocation {
    /// Resolve this process's context, once, before any command runs. It costs
    /// one Git process (§FS-001-config.8.1).
    pub fn resolve() -> Self {
        Self::from_probe(probe(Path::new(".")).as_ref())
    }

    /// Below the root, the root is the way up to it. At the root, outside a
    /// work tree, or when Git cannot answer, it is the invocation directory and
    /// no ancestor is searched (§FS-001-config.8.1).
    fn from_probe(probe: Option<&Probe>) -> Self {
        match probe {
            Some(probe) if !probe.cdup.is_empty() => Self {
                root: PathBuf::from(probe.cdup.trim_end_matches('/')),
                prefix: probe.prefix.clone(),
                toplevel: Some(probe.toplevel.clone()),
            },
            _ => Self {
                root: PathBuf::from("."),
                prefix: String::new(),
                toplevel: None,
            },
        }
    }

    /// The root every command reads, spelled from the invocation directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where the invocation directory sits under the root: empty at the root.
    #[cfg(test)]
    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    /// An explicit `--config`, resolved once against the invocation directory,
    /// so the current tree and every `audit --history` revision read the same
    /// file (§FS-001-config.8.1). Below the root a relative path becomes
    /// absolute: the library joins it to the root, and a command that offers a
    /// rerun echoes it, which then works from where the caller stands. At the
    /// root, outside Git, or when absolute, it is kept as typed.
    pub fn config_path(&self, explicit: Option<PathBuf>) -> Option<PathBuf> {
        let path = explicit?;
        match &self.toplevel {
            Some(top) if !path.is_absolute() => Some(lexical(&top.join(&self.prefix).join(&path))),
            _ => Some(path),
        }
    }
}

/// `..` folded into the step before it, as `audit --history` needs the
/// resolved `--config` to be (§FS-004-check-audit.2.1). A step above an
/// absolute root stays at the root; a relative path keeps its leading `..`.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// How many levels below `root` the caller stands, read from its spelling: a
/// root made only of `..` steps is the invocation context's (§FS-001-config.8.1),
/// and the caller's paths are rebased from there (§FS-007-measure.1). Any other
/// root, `.` included, puts the caller at it.
pub(crate) fn levels_below(root: &Path) -> usize {
    let mut levels = 0;
    for component in root.components() {
        if component != Component::ParentDir {
            return 0;
        }
        levels += 1;
    }
    levels
}

/// The invocation directory under `root`, `sub/deep`, for a caller standing
/// below it: the prefix a path they typed is rebased onto (§FS-007-measure.1).
pub(crate) fn caller_prefix(root: &Path) -> io::Result<String> {
    let top = root.canonicalize()?;
    let here = Path::new(".").canonicalize()?;
    Ok(crate::scan::relative(&top, &here).unwrap_or_default())
}

/// A path with a `..` step that a caller below `root` typed and the kernel
/// found missing, folded lexically from the caller's directory onto the root:
/// `../other/gone.py` from `sub/` is `other/gone.py`, an address measurement
/// reports as a file-level error (§FS-004-check-audit.5) and a stale exact entry
/// is found by. A fold that climbs above the root is refused, naming `raw` as
/// typed (§FS-007-measure.1). Any error but `NotFound`, or a caller directory
/// that cannot be read, is returned as it came. A missing path that steps back
/// with `..` after a symlink folds as written, not where the link leads: the
/// trade the resolved `--config` makes too.
pub(crate) fn fold_missing(root: &Path, raw: &str, error: io::Error) -> io::Result<String> {
    if error.kind() != io::ErrorKind::NotFound {
        return Err(error);
    }
    let Ok(prefix) = caller_prefix(root) else {
        return Err(error);
    };
    let folded = lexical(&Path::new(&prefix).join(raw));
    if folded.components().next() == Some(Component::ParentDir) {
        return Err(outside_repository(raw));
    }
    Ok(folded.to_string_lossy().replace('\\', "/"))
}

/// The refusal of a path that leaves the repository, naming it as the caller
/// typed it (§FS-007-measure.1).
pub(crate) fn outside_repository(raw: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("path `{raw}` is outside the repository"),
    )
}

/// A repository path spelled from where the caller stands, so a command that
/// offers it runs as printed (§FS-001-config.8.1): the inverse of the rebase
/// [`crate::scan::normalize_repo_path`] applies. From `sub/`, `sub/x.py` is
/// `x.py` and `other/x.py` is `../other/x.py`. At the root, and for any root
/// not made only of `..` steps, the path is returned unchanged.
pub(crate) fn spelled_from_caller(root: &Path, path: &str) -> String {
    if levels_below(root) == 0 {
        return path.to_owned();
    }
    match caller_prefix(root) {
        Ok(prefix) => relative_to(&prefix, path),
        // The way up and then down again still runs from where the caller stands.
        Err(_) => document_prefix(root) + path,
    }
}

/// `path` as seen from `prefix`, both repository-relative and written with `/`.
fn relative_to(prefix: &str, path: &str) -> String {
    let here: Vec<&str> = prefix.split('/').filter(|step| !step.is_empty()).collect();
    let there: Vec<&str> = path.split('/').filter(|step| !step.is_empty()).collect();
    let shared = here.iter().zip(&there).take_while(|(a, b)| a == b).count();
    let mut steps = vec![".."; here.len() - shared];
    steps.extend(&there[shared..]);
    if steps.is_empty() {
        crate::scan::REPOSITORY_ROOT.to_owned()
    } else {
        steps.join("/")
    }
}

/// What a path that locates a config document starts with, so it opens from
/// where the caller stands (§FS-001-config.8.1): nothing at the root, so a run
/// there keeps its bytes, and the root with a `/` anywhere else, `../` below
/// it. Written with `/` on every platform, as the spec spells it.
pub(crate) fn document_prefix(root: &Path) -> String {
    if root.as_os_str().is_empty() || root == Path::new(".") {
        return String::new();
    }
    let spelled = root.to_string_lossy().replace('\\', "/");
    if spelled.ends_with('/') {
        spelled
    } else {
        spelled + "/"
    }
}

/// The config in the invocation directory that discovery passes over because
/// it reads the root (§FS-001-config.8.4). Only a caller below the root has
/// one, and only the directory they stand in is looked at. Either home counts;
/// with both, the one discovery would read there is named (§FS-001-config.8.3).
pub(crate) fn passed_over(root: &Path) -> Option<&'static str> {
    if levels_below(root) == 0 {
        return None;
    }
    [CONFIG_HOME, DEPRECATED_CONFIG_HOME]
        .into_iter()
        .find(|home| Path::new(home).exists())
}

/// The root, absolute: which directory was searched is what a run on the
/// built-in defaults has to say (§FS-001-config.8.5). Spelled as audit's
/// empty-scan note spells it (§FS-004-check-audit.2.2).
pub(crate) fn absolute(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
}

/// The warning `init` owes a target below a work tree's root: the config it
/// writes there is not the one discovery reads (§FS-002-init.1). `None` at a
/// root, outside a work tree, or for a target Git cannot answer for. Both paths
/// are absolute, as Git resolves them.
pub fn below_root_warning(target: &Path) -> Option<String> {
    let probe = probe(target)?;
    let prefix = probe.prefix.trim_end_matches('/');
    if prefix.is_empty() {
        return None;
    }
    let top = probe.toplevel.to_string_lossy();
    Some(format!(
        "fissile: warning: {}/{prefix} is below the root of the git repository at {top}; \
         fissile discovers config only at that root, so the config here is not in effect",
        top.trim_end_matches('/')
    ))
}

#[cfg(test)]
#[path = "invocation_tests.rs"]
mod tests;
