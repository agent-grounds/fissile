//! What `check --base <ref>` judges (§FS-004-check-audit.1.5): the two endpoints
//! resolved once to commits, the files whose tree entries differ between them,
//! the removals that difference proves, and each selected file measured from its
//! blob at `HEAD`. Git objects only — the index, the working tree, and every ref
//! are read, never written.

use std::io;
use std::path::Path;

use crate::config::{Scan, Tokens};
use crate::revision;
use crate::scan::{self, MeasuredFile};

/// A gitlink's tree mode: a submodule pointer, with no content to measure.
const GITLINK: &str = "160000";

/// The resolved range a base run judges. `base` and `head` are full SHAs, read
/// once so selection and history count the same commits (§FS-004-check-audit.1.4).
#[derive(Clone, Debug)]
pub(crate) struct Range {
    /// `<ref>` as the caller typed it, for every diagnostic and detail.
    pub expression: String,
    pub base: String,
    pub head: String,
}

/// A selected file: its `HEAD` path and the blob that holds it there.
#[derive(Clone, Debug)]
pub(crate) struct Selected {
    pub path: String,
    pub blob: String,
}

/// The tree difference between the endpoints (§FS-004-check-audit.1.5).
pub(crate) struct Selection {
    pub files: Vec<Selected>,
    /// Paths in `<ref>`'s tree and absent from `HEAD`'s (§FS-004-check-audit.1.3).
    pub removed: Vec<String>,
}

/// Resolve `<ref>` and `HEAD` and prove the first an ancestor of the second,
/// or fail run-level with the one diagnostic naming why (§FS-004-check-audit.1.5,
/// §FS-004-check-audit.5). fissile never fetches; a missing ref is told to.
pub(crate) fn resolve(root: &Path, expression: &str) -> io::Result<Range> {
    let fail = |cause: String| io::Error::other(format!("--base {expression}: {cause}"));
    let git = |error: io::Error| fail(format!("git is unavailable: {error}"));
    if !revision::inside_work_tree(root).map_err(git)? {
        return Err(fail(format!(
            "not a git work tree: {} (run inside a repository)",
            root.display()
        )));
    }
    let Some(head) = revision::resolve_commit(root, "HEAD").map_err(git)? else {
        return Err(fail("HEAD names no commit yet; commit first".to_owned()));
    };
    let Some(base) = revision::resolve_commit(root, expression).map_err(git)? else {
        return Err(fail(unresolved(root, expression).map_err(git)?));
    };
    if !revision::is_ancestor(root, &base, &head).map_err(git)? {
        return Err(fail(
            unproven_ancestry(root, expression, &base, &head).map_err(git)?,
        ));
    }
    Ok(Range {
        expression: expression.to_owned(),
        base,
        head,
    })
}

/// Why `<ref>` is not proven an ancestor of `HEAD`. A shallow clone cannot tell
/// a divergent base from a cut graph, and its merge-base may be empty, so it is
/// told to fetch full history rather than given a fork point
/// (§FS-004-check-audit.1.5).
fn unproven_ancestry(root: &Path, expression: &str, base: &str, head: &str) -> io::Result<String> {
    if revision::is_shallow(root)? {
        return Ok(format!(
            "cannot prove {base} an ancestor of HEAD {head} in a shallow clone; fetch full \
             history (git fetch --unshallow, or fetch-depth: 0 in CI) and run again"
        ));
    }
    Ok(format!(
        "{base} is not an ancestor of HEAD {head}; to check against the fork point, \
         run fissile check --base \"$(git merge-base {expression} HEAD)\""
    ))
}

/// Why an expression named no single commit: a range, an object that is not a
/// commit, or nothing this repository has.
fn unresolved(root: &Path, expression: &str) -> io::Result<String> {
    if expression.contains("..") {
        return Ok("names a range, not one commit; pass its base alone".to_owned());
    }
    if revision::names_object(root, expression)? {
        return Ok("names an object that is not a commit".to_owned());
    }
    Ok(
        "names no commit in this repository; fetch it first if it is a remote ref \
         (fissile never fetches)"
            .to_owned(),
    )
}

/// The files whose entries differ between the endpoints' trees, compared
/// directly and never through a merge-base, plus the removals that difference
/// proves (§FS-004-check-audit.1.5). `[scan].exclude` applies; `[scan].include`
/// and gitignore do not (§FS-001-config.2).
pub(crate) fn select(root: &Path, range: &Range, scan_config: &Scan) -> io::Result<Selection> {
    let output = revision::git(
        root,
        &[
            "diff-tree",
            "-r",
            "-z",
            "--raw",
            "--no-renames",
            "--no-abbrev",
            &range.base,
            &range.head,
            "--",
        ],
    )?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let first = detail.lines().next().unwrap_or("").trim();
        return Err(io::Error::other(format!(
            "--base {}: git diff-tree failed: {first}",
            range.expression
        )));
    }
    let mut selection = parse_raw_diff(&output.stdout);
    let exclude = scan::compile_globs(&scan_config.exclude);
    selection
        .files
        .retain(|file| !scan::is_excluded(&file.path, &exclude));
    Ok(selection)
}

/// Parse `git diff-tree -r -z --raw --no-renames`. Without rename detection a
/// rename is a deletion of its source and an addition of its destination, which
/// is exactly the split selection and removal want.
fn parse_raw_diff(raw: &[u8]) -> Selection {
    let text = String::from_utf8_lossy(raw);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut selection = Selection {
        files: Vec::new(),
        removed: Vec::new(),
    };
    while let Some(meta) = fields.next() {
        let Some(path) = fields.next() else { break };
        // `:<old mode> <new mode> <old blob> <new blob> <status>`
        let meta: Vec<&str> = meta.trim_start_matches(':').split_whitespace().collect();
        let [_, new_mode, _, new_blob, status] = meta[..] else {
            continue;
        };
        match status.as_bytes().first() {
            // Type changes are included on purpose: a link turned into a file is
            // new content at `HEAD`. A gitlink has none (§FS-004-check-audit.1.5).
            Some(b'A' | b'M' | b'T') if new_mode != GITLINK => selection.files.push(Selected {
                path: path.to_owned(),
                blob: new_blob.to_owned(),
            }),
            Some(b'D') => selection.removed.push(path.to_owned()),
            _ => {}
        }
    }
    selection
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    selection.removed.sort();
    selection
}

/// Measure each selected file from its `HEAD` blob, whatever the index or the
/// working tree holds. A blob that cannot be read or measured is a file-level
/// error, never a run-level one (§FS-004-check-audit.5).
pub(crate) fn measure_each(
    root: &Path,
    files: &[Selected],
    tokens: &Tokens,
) -> (Vec<MeasuredFile>, Vec<String>) {
    let mut measured = Vec::with_capacity(files.len());
    let mut errors = Vec::new();
    for file in files {
        match measure_blob(root, file, tokens) {
            Ok(measurement) => measured.push(measurement),
            Err(error) => errors.push(scan::measure_error_line(&file.path, &error)),
        }
    }
    (measured, errors)
}

fn measure_blob(root: &Path, file: &Selected, tokens: &Tokens) -> io::Result<MeasuredFile> {
    let output = revision::git(root, &["cat-file", "blob", &file.blob])?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let first = detail.lines().next().unwrap_or("").trim();
        return Err(io::Error::other(format!("git cat-file failed: {first}")));
    }
    scan::measure_blob_with_context(root, &file.path, &output.stdout, tokens)
}

/// Why a base run failed, for its epilogue (§FS-004-check-audit.1.2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reason {
    Overflow,
    PromotedSoft,
    DeadEntry,
    Unmeasured,
}

/// The base epilogue: the lead on its own line, `<ref>` as typed, then the
/// remedy wrapped at 78 columns. Nothing is staged and no commit is being made,
/// so it names neither the hook, nor `--no-verify`, nor unstaging
/// (§FS-004-check-audit.1.2).
pub(crate) fn epilogue(expression: &str, reason: Reason) -> String {
    let remedy = match reason {
        Reason::Overflow => "Split the file, or ask a human for a reviewed hard exception.",
        Reason::PromotedSoft => {
            "Split now or record the debt with `fissile exception add <path> --severity \
             soft --rule <rule> --kind <kind>`."
        }
        Reason::DeadEntry => {
            "Remove the exception entry above, or point it at the path its file moved to."
        }
        Reason::Unmeasured => {
            "A selected file could not be measured, so nothing above accounts for it — \
             fix the path the error names."
        }
    };
    format!(
        "`fissile check --base {expression}` rejected the selected HEAD files.\n{}",
        crate::report::wrap(remedy, 78).join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_diff_selects_content_and_proves_removals() {
        let blob = "b".repeat(40);
        let zero = "0".repeat(40);
        let raw = format!(
            ":000000 100644 {zero} {blob} A\0added.md\0\
             :100644 100644 {blob} {blob} M\0changed.md\0\
             :120000 100644 {blob} {blob} T\0link.md\0\
             :160000 160000 {blob} {blob} M\0vendor/sub\0\
             :100644 000000 {blob} {zero} D\0gone.md\0"
        );
        let selection = parse_raw_diff(raw.as_bytes());
        let paths: Vec<&str> = selection.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["added.md", "changed.md", "link.md"]);
        assert_eq!(selection.removed, ["gone.md"]);
    }

    #[test]
    fn epilogue_leads_alone_and_wraps_its_remedy() {
        let text = epilogue("origin/main", Reason::PromotedSoft);
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some("`fissile check --base origin/main` rejected the selected HEAD files.")
        );
        assert!(lines.all(|line| line.chars().count() <= 78), "{text}");
        assert!(!text.contains("--no-verify") && !text.contains("staged"));
    }
}
