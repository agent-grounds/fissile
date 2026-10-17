//! Revision plumbing shared by every command that names commits: resolving a
//! caller's expression to one commit, and proving ancestry
//! (§FS-004-check-audit.1.5, §FS-004-check-audit.2.1). Callers own their
//! diagnostics; these only say what Git answered.

use std::io;
use std::path::Path;
use std::process::{Command, Output};

/// Run one Git command in `root`, capturing its output.
pub(crate) fn git(root: &Path, args: &[&str]) -> io::Result<Output> {
    Command::new("git").arg("-C").arg(root).args(args).output()
}

/// Whether `root` is inside a Git work tree.
pub(crate) fn inside_work_tree(root: &Path) -> io::Result<bool> {
    let output = git(root, &["rev-parse", "--is-inside-work-tree"])?;
    Ok(output.status.success() && output.stdout == b"true\n")
}

/// The full SHA of the commit `expression` names, or `None` when it names no
/// single commit. `--end-of-options` makes a caller-controlled revision data,
/// never an option.
pub(crate) fn resolve_commit(root: &Path, expression: &str) -> io::Result<Option<String>> {
    let requested = format!("{expression}^{{commit}}");
    let output = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &requested],
    )?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned()))
}

/// Whether `expression` names any object at all, commit or not.
pub(crate) fn names_object(root: &Path, expression: &str) -> io::Result<bool> {
    let output = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--end-of-options",
            expression,
        ],
    )?;
    Ok(output.status.success())
}

/// Whether `root`'s repository is a shallow clone, whose cut graph can hide an
/// ancestry that holds. An unanswered probe reads as not shallow.
pub(crate) fn is_shallow(root: &Path) -> io::Result<bool> {
    let output = git(root, &["rev-parse", "--is-shallow-repository"])?;
    Ok(output.status.success() && output.stdout == b"true\n")
}

/// Whether `ancestor` is reachable from `descendant`, equality included. Both
/// are resolved SHAs.
pub(crate) fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> io::Result<bool> {
    let output = git(
        root,
        &["merge-base", "--is-ancestor", "--", ancestor, descendant],
    )?;
    Ok(output.status.success())
}
