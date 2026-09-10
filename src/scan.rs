//! Filesystem traversal and measurement (§FS-004-check-audit): turn a config's
//! scan scope or a git-staged set into [`FileMeasurement`]s, honoring
//! `[scan].exclude`, `respect_gitignore`, and the opt-in `[tokens].command`.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Component;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{Scan, Tokens};
use crate::{FileMeasurement, Glob, measure_bytes, measure_text};

/// A measurement plus the encoding fact needed to describe a line finding
/// truthfully. The encoding is kept out of the public [`FileMeasurement`] API;
/// command renderers use it only while carrying one scan through evaluation.
pub(crate) struct MeasuredFile {
    pub measurement: FileMeasurement,
    pub utf8: bool,
}

/// Compile a list of glob patterns once for repeated matching.
pub fn compile_globs(patterns: &[String]) -> Vec<Glob> {
    patterns.iter().map(Glob::new).collect()
}

/// Whether a repo-relative `/`-path is removed by `[scan].exclude`.
pub fn is_excluded(path: &str, exclude: &[Glob]) -> bool {
    exclude.iter().any(|glob| glob.matches(path))
}

/// Walk the configured scan scope and return repo-relative paths to measure,
/// after exclusion and gitignore filtering (§FS-001-config.2).
pub fn walk_scope(root: &Path, scan: &Scan) -> io::Result<Vec<String>> {
    let exclude = compile_globs(&scan.exclude);
    let mut out = Vec::new();

    for entry in &scan.include {
        if has_glob_meta(entry) {
            let include = Glob::new(entry);
            let base = root.join(glob_literal_prefix(entry));
            if base.is_file() {
                if let Some(rel) = relative(root, &base)
                    && include.matches(&rel)
                {
                    push_if_kept(&mut out, &rel, &exclude);
                }
            } else if base.is_dir() {
                walk(root, base, Some(&include), &exclude, scan, &mut out)?;
            }
        } else {
            let full = root.join(entry);
            if full.is_file() {
                push_if_kept(&mut out, entry, &exclude);
            } else if full.is_dir() {
                walk(root, full, None, &exclude, scan, &mut out)?;
            }
        }
    }

    // Files that are themselves ignored but sit in a kept directory; the
    // ignored *directories* are already gone (§FS-001-config.2).
    if scan.respect_gitignore {
        filter_gitignored(root, &mut out);
    }

    out.sort();
    out.dedup();
    Ok(out)
}

/// Breadth-first walk from `start`, keeping files that pass `include` and
/// `exclude`. Ignored directories are pruned per level, so an ignored subtree
/// is never entered and git calls track depth, not count (§FS-001-config.2).
fn walk(
    root: &Path,
    start: PathBuf,
    include: Option<&Glob>,
    exclude: &[Glob],
    scan: &Scan,
    out: &mut Vec<String>,
) -> io::Result<()> {
    let mut level = vec![start];

    while !level.is_empty() {
        let mut next = Vec::new();

        for dir in &level {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                let Some(rel) = relative(root, &path) else {
                    continue;
                };
                let file_type = entry.file_type()?;
                if file_type.is_dir() {
                    // Skip VCS metadata always, and prune directories whose whole
                    // subtree is excluded (e.g. `target/**`).
                    if rel == ".git" || is_excluded(&format!("{rel}/"), exclude) {
                        continue;
                    }
                    next.push(path);
                } else if file_type.is_file() && include.is_none_or(|include| include.matches(&rel))
                {
                    push_if_kept(out, &rel, exclude);
                }
            }
        }

        if scan.respect_gitignore {
            retain_unignored(root, &mut next);
        }
        level = next;
    }

    Ok(())
}

/// Drop the directories git ignores, so their subtrees are never walked.
fn retain_unignored(root: &Path, dirs: &mut Vec<PathBuf>) {
    if dirs.is_empty() {
        return;
    }
    let rels: Vec<String> = dirs.iter().filter_map(|dir| relative(root, dir)).collect();
    if rels.len() != dirs.len() {
        return; // A path outside the root: leave the level untouched.
    }
    let ignored = git_ignored(root, &rels);
    if ignored.is_empty() {
        return;
    }
    let mut keep = rels.iter().map(|rel| !ignored.contains(rel));
    dirs.retain(|_| keep.next().unwrap_or(true));
}

fn has_glob_meta(path: &str) -> bool {
    path.contains(['*', '?', '['])
}

fn glob_literal_prefix(pattern: &str) -> PathBuf {
    let mut prefix = PathBuf::new();
    for segment in pattern.split('/') {
        if segment.is_empty() || has_glob_meta(segment) {
            break;
        }
        prefix.push(segment);
    }
    prefix
}

fn push_if_kept(out: &mut Vec<String>, rel: &str, exclude: &[Glob]) {
    if !is_excluded(rel, exclude) {
        out.push(rel.to_owned());
    }
}

fn relative(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

/// Normalize an explicit file path to the repo-relative `/` form used by rules
/// and exact exceptions (§FS-003-exceptions.3, §FS-004-check-audit.1).
pub fn normalize_repo_path(root: &Path, raw: &str) -> io::Result<String> {
    let raw = raw.replace('\\', "/");
    let path = Path::new(&raw);
    if path.is_absolute()
        || path
            .components()
            .any(|component| component == Component::ParentDir)
    {
        let root = root.canonicalize()?;
        let full = if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        }
        .canonicalize()
        // Name the argument: the caller may have passed several (§FS-004-check-audit.5).
        .map_err(|error| io::Error::new(error.kind(), format!("{raw}: {error}")))?;
        return relative(&root, &full).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("path `{raw}` is outside the repository"),
            )
        });
    }

    clean_relative_path(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("path `{raw}` is not a repo-relative file path"),
        )
    })
}

fn clean_relative_path(path: &Path) -> Option<String> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                out.pop();
            }
            Component::Prefix(_) | Component::RootDir => return None,
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out.to_string_lossy().replace('\\', "/"))
    }
}

/// The git-staged file set for `check --staged` (§FS-004-check-audit.1). Deleted
/// paths are dropped: there is nothing left to measure. Returns the paths with
/// `[scan].exclude` applied.
pub fn staged_files(root: &Path, scan: &Scan) -> io::Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["diff", "--cached", "--name-only", "--diff-filter=ACMR"])
        .output()?;
    if !output.status.success() {
        // Outside a repository `git diff` falls into no-index mode and blames
        // the `--cached` flag; probe for the real cause so the diagnostic says
        // "not a git repository" instead (§FS-004-check-audit.5).
        let in_repo = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map(|probe| probe.status.success())
            .unwrap_or(false);
        if !in_repo {
            return Err(io::Error::other(format!(
                "not a git repository: {} (run inside a repo, or pass paths)",
                root.display()
            )));
        }
        return Err(io::Error::other(git_failure(
            "git diff --cached",
            &output.stderr,
        )));
    }

    let exclude = compile_globs(&scan.exclude);
    let mut out: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| !is_excluded(line, &exclude))
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    Ok(out)
}

/// The paths this commit removes: staged deletions, and the old side of a
/// staged rename (§FS-004-check-audit.1.3). What proves an entry has outlived
/// its file — a path merely absent from the tree was removed from nothing.
pub fn staged_removals(root: &Path) -> io::Result<Vec<String>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--diff-filter=DR",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(git_failure(
            "git diff --cached",
            &output.stderr,
        )));
    }

    // `-z` writes every field NUL-separated and verbatim. Without it git applies
    // `core.quotePath`, so a removed `src/café.rs` arrives as `"src/caf\303\251.rs"`
    // and matches no entry — silence at the one moment this check exists for.
    let text = String::from_utf8_lossy(&output.stdout);
    let mut fields = text.split('\0').filter(|field| !field.is_empty());
    let mut out: Vec<String> = Vec::new();
    while let Some(status) = fields.next() {
        // `D` is followed by one path; `R<score>` by the old path then the new
        // one. The old path is what an entry was pointing at.
        let Some(path) = fields.next() else { break };
        if status.starts_with('R') || status.starts_with('C') {
            fields.next();
        }
        if status.starts_with('D') || status.starts_with('R') {
            out.push(path.to_owned());
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// `<command> failed` plus git's first stderr line, when there is one.
fn git_failure(command: &str, stderr: &[u8]) -> String {
    let detail = String::from_utf8_lossy(stderr);
    match detail
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(line) => format!("{command} failed: {line}"),
        None => format!("{command} failed"),
    }
}

fn filter_gitignored(root: &Path, paths: &mut Vec<String>) {
    if paths.is_empty() {
        return;
    }
    let ignored = git_ignored(root, paths);
    paths.retain(|path| !ignored.contains(path));
}

/// The subset of `paths` that git ignores, resolved in one `check-ignore` call.
/// Best-effort by design: an unavailable or failing git yields an empty set, so
/// the caller keeps its exclusion-only result rather than losing files.
fn git_ignored(root: &Path, paths: &[String]) -> HashSet<String> {
    let mut child = Command::new("git");
    child
        .arg("-C")
        .arg(root)
        .args(["check-ignore", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());

    let Ok(mut handle) = child.spawn() else {
        return HashSet::new();
    };
    if let Some(mut stdin) = handle.stdin.take() {
        use io::Write;
        let _ = stdin.write_all(paths.join("\n").as_bytes());
    }
    let Ok(output) = handle.wait_with_output() else {
        return HashSet::new();
    };
    // exit 0 = some ignored, 1 = none ignored; anything else: report nothing.
    if !matches!(output.status.code(), Some(0 | 1)) {
        return HashSet::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Measure one repo-relative file. UTF-8 files are line-classified; others use
/// raw physical lines alongside bytes. Token counts come from the opt-in
/// external command (§DA-001-token-external-command).
pub fn measure_file(root: &Path, rel: &str, tokens: &Tokens) -> io::Result<FileMeasurement> {
    Ok(measure_file_with_context(root, rel, tokens)?.measurement)
}

/// Measure one file while retaining whether its line breakdown came from UTF-8
/// classification or the raw-byte fallback.
pub(crate) fn measure_file_with_context(
    root: &Path,
    rel: &str,
    tokens: &Tokens,
) -> io::Result<MeasuredFile> {
    let bytes = fs::read(root.join(rel))?;
    let mut measured = measure_content(rel, &bytes);
    if tokens.enabled
        && let Some(count) = run_token_command(root, &tokens.command, rel)?
    {
        measured.measurement = measured.measurement.with_tokens(count);
    }
    Ok(measured)
}

/// Measure a staged blob for `check --staged` (§FS-004-check-audit.1).
pub fn measure_staged_file(root: &Path, rel: &str, tokens: &Tokens) -> io::Result<FileMeasurement> {
    Ok(measure_staged_file_with_context(root, rel, tokens)?.measurement)
}

/// Measure a staged blob while retaining its encoding for text rendering.
pub(crate) fn measure_staged_file_with_context(
    root: &Path,
    rel: &str,
    tokens: &Tokens,
) -> io::Result<MeasuredFile> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("show")
        .arg(format!(":{rel}"))
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(git_failure("git show", &output.stderr)));
    }
    let mut measured = measure_content(rel, &output.stdout);
    if tokens.enabled
        && let Some(count) =
            run_token_command_for_staged_bytes(root, &tokens.command, rel, &output.stdout)?
    {
        measured.measurement = measured.measurement.with_tokens(count);
    }
    Ok(measured)
}

fn measure_content(rel: &str, bytes: &[u8]) -> MeasuredFile {
    match std::str::from_utf8(bytes) {
        Ok(text) => MeasuredFile {
            measurement: measure_text(rel, text),
            utf8: true,
        },
        // Non-UTF-8 content still measures lines — raw physical lines, all
        // counted as content — so a stray encoding never errors the commit
        // gate (§FS-001-config.3.1, §FS-004-check-audit.5).
        Err(_) => MeasuredFile {
            measurement: measure_bytes(rel, bytes).with_lines(count_raw_lines(bytes)),
            utf8: false,
        },
    }
}

/// Measure a historical Git blob with the same byte, line, and optional token
/// policy used for the staged blob (§FS-004-check-audit.1.4). Git object
/// retrieval is batched by the caller; only an opt-in token command may need a
/// process for this individual version.
pub(crate) fn measure_history_blob(
    root: &Path,
    rel: &str,
    bytes: &[u8],
    tokens: &Tokens,
) -> io::Result<FileMeasurement> {
    let mut measured = measure_content(rel, bytes);
    if tokens.enabled
        && let Some(count) = run_token_command_for_staged_bytes(root, &tokens.command, rel, bytes)?
    {
        measured.measurement = measured.measurement.with_tokens(count);
    }
    Ok(measured.measurement)
}

/// Physical lines in arbitrary bytes: `\n` count plus an unterminated tail.
fn count_raw_lines(bytes: &[u8]) -> u64 {
    let newlines = bytes.iter().filter(|&&byte| byte == b'\n').count() as u64;
    match bytes.last() {
        None => 0,
        Some(b'\n') => newlines,
        Some(_) => newlines + 1,
    }
}

/// The one-line diagnostic for a path that could not be measured
/// (§FS-004-check-audit.5). Directories get a hint instead of a bare OS error.
pub fn measure_error_line(rel: &str, error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::IsADirectory {
        return format!("cannot measure {rel}: is a directory (pass files, or run fissile audit)");
    }
    format!("cannot measure {rel}: {error}")
}

/// Run `[tokens].command` with `{path}` substituted and parse one integer
/// (§FS-001-config.7).
fn run_token_command(root: &Path, command: &[String], rel: &str) -> io::Result<Option<u64>> {
    run_token_command_with_path(root, command, rel, rel)
}

fn run_token_command_for_staged_bytes(
    root: &Path,
    command: &[String],
    rel: &str,
    bytes: &[u8],
) -> io::Result<Option<u64>> {
    if command.is_empty() {
        return Ok(None);
    }
    let path = staged_temp_path(rel);
    fs::write(&path, bytes)?;
    let path_arg = path.to_string_lossy().into_owned();
    let result = run_token_command_with_path(root, command, &path_arg, rel);
    let _ = fs::remove_file(path);
    result
}

fn staged_temp_path(rel: &str) -> PathBuf {
    let sanitized: String = rel
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect();
    std::env::temp_dir().join(format!("fissile-staged-{}-{sanitized}", std::process::id()))
}

fn run_token_command_with_path(
    root: &Path,
    command: &[String],
    path_arg: &str,
    rel: &str,
) -> io::Result<Option<u64>> {
    let Some((program, rest)) = command.split_first() else {
        return Ok(None);
    };
    let args: Vec<String> = rest
        .iter()
        .map(|arg| arg.replace("{path}", path_arg))
        .collect();
    let output = Command::new(program)
        .args(&args)
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("token command failed for {rel}")));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.split_whitespace()
        .next()
        .and_then(|token| token.parse::<u64>().ok())
        .map(|count| Ok(Some(count)))
        .unwrap_or_else(|| {
            Err(io::Error::other(format!(
                "token command did not print an integer for {rel}"
            )))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn globs(patterns: &[&str]) -> Vec<Glob> {
        patterns.iter().map(|p| Glob::new(*p)).collect()
    }

    fn temp_root() -> std::path::PathBuf {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("fissile-scan-{}-{n}", std::process::id()))
    }

    #[test]
    fn binary_content_measures_raw_lines() {
        // §FS-001-config.3.1: non-UTF-8 bytes still get a physical line count.
        let measurement = measure_content("src/odd.rs", b"a\xff\nb\xfe\nc").measurement;
        let lines = measurement.lines.expect("lines are measured");
        assert_eq!(lines.total, 3);
        assert_eq!((lines.blank, lines.comment), (0, 0));
        assert_eq!(count_raw_lines(b""), 0);
        assert_eq!(count_raw_lines(b"x\n"), 1);
    }

    #[test]
    fn measure_error_lines_name_the_path() {
        // §FS-004-check-audit.5: the diagnostic carries the path, and a
        // directory gets a hint instead of a bare OS error.
        let missing = io::Error::from(io::ErrorKind::NotFound);
        let line = measure_error_line("src/gone.rs", &missing);
        assert!(line.starts_with("cannot measure src/gone.rs:"));
        let dir = io::Error::from(io::ErrorKind::IsADirectory);
        assert_eq!(
            measure_error_line("src", &dir),
            "cannot measure src: is a directory (pass files, or run fissile audit)"
        );
    }

    #[test]
    fn exclusion_matches_subtrees_and_extensions() {
        let exclude = globs(&["target/**", "**/*.lock"]);
        assert!(is_excluded("target/debug/app", &exclude));
        assert!(is_excluded("Cargo.lock", &exclude));
        assert!(!is_excluded("src/lib.rs", &exclude));
    }

    #[test]
    fn directory_prune_test_matches_doublestar_excludes() {
        let exclude = globs(&["target/**"]);
        assert!(is_excluded("target/", &exclude));
        assert!(!is_excluded("src/", &exclude));
    }

    /// An ignored directory must be pruned, not filtered afterwards
    /// (§FS-001-config.2). Proving a subtree was never entered needs one that
    /// fails when entered: an unreadable directory errors a descending walk.
    #[cfg(unix)]
    #[test]
    fn gitignored_directories_are_never_descended_into() {
        use std::os::unix::fs::PermissionsExt;

        let root = temp_root();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("ignored/unreadable")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn lib() {}\n").unwrap();
        fs::write(root.join("ignored/big.rs"), "fn big() {}\n").unwrap();
        fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .expect("git runs")
        };
        git(&["init", "-q"]);
        fs::set_permissions(
            root.join("ignored/unreadable"),
            fs::Permissions::from_mode(0o000),
        )
        .unwrap();

        let scan = Scan {
            include: vec![".".to_owned()],
            exclude: Vec::new(),
            respect_gitignore: true,
        };
        let paths = walk_scope(&root, &scan).expect("ignored subtree is pruned, not entered");

        assert!(paths.contains(&"src/lib.rs".to_owned()));
        assert!(!paths.iter().any(|path| path.starts_with("ignored/")));

        fs::set_permissions(
            root.join("ignored/unreadable"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn walk_scope_expands_include_globs() {
        let root = temp_root();
        fs::create_dir_all(root.join("src/nested")).unwrap();
        fs::write(root.join("src/lib.rs"), "fn lib() {}\n").unwrap();
        fs::write(root.join("src/nested/mod.rs"), "fn nested() {}\n").unwrap();
        fs::write(root.join("src/readme.md"), "# src\n").unwrap();

        let paths = walk_scope(
            &root,
            &Scan {
                include: vec!["src/**/*.rs".to_owned()],
                exclude: Vec::new(),
                respect_gitignore: false,
            },
        )
        .expect("walks scope");

        assert_eq!(paths, vec!["src/lib.rs", "src/nested/mod.rs"]);
        let _ = fs::remove_dir_all(root);
    }
}
