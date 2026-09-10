//! Fixture-driven end-to-end suite (§GOAL-003-friendly-output.3). Every directory
//! under `tests/e2e/cases/` named `E2E-*` is one scenario: a `case.toml` manifest plus a
//! `repo/` working tree. The harness drives the real `fissile` binary against a
//! throwaway copy of the tree and asserts the exit code and output the manifest
//! declares, so each documented behavior under `docs/functional-spec/` has at least
//! one executable scenario.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

mod soft_edit_limit;

/// One scenario manifest (`case.toml`). Unknown keys are rejected: a misspelled
/// key would otherwise drop the assertion it names, and a dropped assertion is a
/// passing case that checks nothing.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    /// Arguments passed to `fissile`.
    args: Vec<String>,
    /// Expected process exit code.
    #[serde(default)]
    exit: i32,
    /// Initialize a git repo and stage the tree before running (for `--staged`
    /// checks and the pre-commit hook install).
    #[serde(default)]
    git: bool,
    /// Paths whose removal this commit stages: the tree is committed first, then
    /// each path is `git rm`-ed. The shape of a commit that deletes or moves a
    /// file, which is not the same as a file simply missing from the working
    /// tree (§FS-004-check-audit.1.3). Requires `git`.
    #[serde(default)]
    git_removes: Vec<String>,
    /// Substrings that must appear in stdout.
    #[serde(default)]
    stdout_contains: Vec<String>,
    /// Exact stdout (trailing newline ignored), when the bytes must be stable.
    stdout_equals: Option<String>,
    /// Substrings that must appear in stderr.
    #[serde(default)]
    stderr_contains: Vec<String>,
    /// Repo-relative paths that must exist after the run (for `init`).
    #[serde(default)]
    creates: Vec<String>,
    /// Repo-relative paths that must *not* exist after the run: the shape of a
    /// run that declined to write something it otherwise would have, which
    /// `creates` cannot express (§FS-002-init.6).
    // Each path's directory has to exist after the run. Under a directory
    // nothing created, no path could have been written whatever the command
    // did, so the assertion would pass for a reason the case never meant: name
    // the shallowest path that must not appear instead.
    #[serde(default)]
    absent: Vec<String>,
    /// Assertions on a file's bytes after the run. Stdout says what a command
    /// claims it did; these say what it actually wrote (§FS-008-exception-retune.3).
    #[serde(default)]
    files: Vec<FileAssert>,
    /// Paths that must be symbolic links after the run, and what each must
    /// point at (§FS-002-init.3). Windows refuses links without Developer Mode,
    /// so a case using this sets `unix_only`.
    #[serde(default)]
    links: Vec<LinkAssert>,
    /// Skip on non-Unix hosts. Used by the token-unit case, whose external
    /// counter is a POSIX shell stub (§DA-001-token-external-command); token
    /// mode itself is cross-platform, but a portable counter stub is not, so
    /// this single fixture is Unix-gated the way the bench job is Linux-only
    /// (§DA-002-instruction-count-benchmarks).
    #[serde(default)]
    unix_only: bool,
}

/// One post-run assertion that a path is a link, and where it goes.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkAssert {
    /// Repo-relative path within the throwaway tree.
    path: String,
    /// The exact link target, as stored — relative, so the tree can move.
    target: String,
}

/// One post-run assertion about a file the command wrote. Needles are compared
/// against the raw bytes as text, so a `\r` in a needle asserts the line ending.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileAssert {
    /// Repo-relative path within the throwaway tree.
    path: String,
    #[serde(default)]
    contains: Vec<String>,
    /// Substrings that must be absent — the shape of "this line was not touched".
    #[serde(default)]
    excludes: Vec<String>,
}

/// Output with `\` folded to `/`, so one manifest asserts the same path on
/// every platform. Paths are the only place fissile emits a backslash.
fn separator_agnostic(text: &str) -> String {
    text.replace('\\', "/")
}

/// Whether a manifest path stays inside the throwaway tree. `work.join(path)`
/// on an absolute one yields the absolute path itself, so `/etc/passwd` would be
/// asserted against the host filesystem rather than against the run.
fn is_repo_relative(relative: &str) -> bool {
    !Path::new(relative).components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    })
}

fn cases_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/e2e/cases")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Run one case; return Err(reason) on failure so the caller can report all.
fn run_case(dir: &Path) -> Result<(), String> {
    let manifest = fs::read_to_string(dir.join("case.toml"))
        .map_err(|error| format!("reading case.toml: {error}"))?;
    let case: Case =
        toml::from_str(&manifest).map_err(|error| format!("parsing case.toml: {error}"))?;

    if case.unix_only && !cfg!(unix) {
        return Ok(());
    }

    // Every asserted path is repo-relative by contract; one that escapes would
    // be judged against the host filesystem instead of the run.
    let declared = case
        .creates
        .iter()
        .chain(&case.absent)
        .chain(case.files.iter().map(|assertion| &assertion.path))
        .chain(case.links.iter().map(|assertion| &assertion.path));
    for relative in declared {
        if !is_repo_relative(relative) {
            return Err(format!("{relative} is not a repo-relative path"));
        }
    }

    let work = std::env::temp_dir().join(format!(
        "fissile-e2e-{}-{}",
        std::process::id(),
        dir.file_name().unwrap().to_string_lossy()
    ));
    let _ = fs::remove_dir_all(&work);
    let repo = dir.join("repo");
    if repo.is_dir() {
        copy_tree(&repo, &work);
    } else {
        fs::create_dir_all(&work).unwrap();
    }

    // The removal has to be staged against a commit, so a case that asks for one
    // without a repository would run with no removal staged at all — and could
    // pass for the wrong reason (§FS-004-check-audit.1.3).
    if !case.git_removes.is_empty() && !case.git {
        return Err("git_removes requires git = true".to_owned());
    }

    if case.git {
        git(&work, &["init", "-q"]);
        git(&work, &["add", "-A"]);
        if !case.git_removes.is_empty() {
            // The developer's own git configuration is neutralized: a global
            // signing key or hooks path would fail this commit for reasons that
            // have nothing to do with what the case asserts.
            git(
                &work,
                &[
                    "-c",
                    "user.email=e2e@fissile.invalid",
                    "-c",
                    "user.name=e2e",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                    "core.hooksPath=e2e-no-hooks",
                    "commit",
                    "-q",
                    "-m",
                    "base",
                ],
            );
            for path in &case.git_removes {
                git(&work, &["rm", "-q", path]);
            }
        }
    }

    let args: Vec<&str> = case.args.iter().map(String::as_str).collect();
    let output = Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(&work)
        .args(&args)
        .output()
        .map_err(|error| format!("spawning fissile: {error}"))?;

    // Windows renders `root.join(rel)` with a backslash, so `wrote ./AGENTS.md`
    // arrives as `wrote .\AGENTS.md`. Needles are written the one way and
    // compared against the separator this host happens to use.
    let stdout = separator_agnostic(&String::from_utf8_lossy(&output.stdout));
    let stderr = separator_agnostic(&String::from_utf8_lossy(&output.stderr));
    let code = output.status.code().unwrap_or(-1);
    let mut problems = Vec::new();

    if code != case.exit {
        problems.push(format!("exit {code}, expected {}", case.exit));
    }
    if let Some(expected) = &case.stdout_equals
        && stdout.trim_end_matches('\n') != expected.trim_end_matches('\n')
    {
        problems.push(format!(
            "stdout != expected\n--- got ---\n{stdout}\n--- want ---\n{expected}"
        ));
    }
    for needle in &case.stdout_contains {
        if !stdout.contains(needle) {
            problems.push(format!("stdout missing {needle:?}"));
        }
    }
    for needle in &case.stderr_contains {
        if !stderr.contains(needle) {
            problems.push(format!("stderr missing {needle:?}"));
        }
    }
    for relative in &case.creates {
        if !work.join(relative).exists() {
            problems.push(format!("expected {relative} to be created"));
        }
    }
    for relative in &case.absent {
        let path = work.join(relative);
        if !path.parent().is_none_or(Path::is_dir) {
            problems.push(format!(
                "absent {relative}: its directory does not exist, so nothing could have written it"
            ));
        } else if path.exists() {
            problems.push(format!("expected {relative} not to be written"));
        }
    }
    for assertion in &case.files {
        match fs::read(work.join(&assertion.path)) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                for needle in &assertion.contains {
                    if !text.contains(needle.as_str()) {
                        problems.push(format!("{} missing {needle:?}", assertion.path));
                    }
                }
                for needle in &assertion.excludes {
                    if text.contains(needle.as_str()) {
                        problems.push(format!("{} still contains {needle:?}", assertion.path));
                    }
                }
            }
            Err(error) => problems.push(format!("reading {}: {error}", assertion.path)),
        }
    }
    for assertion in &case.links {
        let path = work.join(&assertion.path);
        // `symlink_metadata` does not follow, so a regular file the run wrote
        // instead of a link is caught rather than silently passing.
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.file_type().is_symlink() => {
                problems.push(format!(
                    "{} is a regular file, expected a link",
                    assertion.path
                ));
            }
            Ok(_) => match fs::read_link(&path) {
                Ok(target) if target != Path::new(&assertion.target) => problems.push(format!(
                    "{} points at {}, expected {}",
                    assertion.path,
                    target.display(),
                    assertion.target
                )),
                Ok(_) => {}
                Err(error) => problems.push(format!("reading link {}: {error}", assertion.path)),
            },
            Err(error) => problems.push(format!("stat {}: {error}", assertion.path)),
        }
    }

    let _ = fs::remove_dir_all(&work);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "\n  stdout:\n{stdout}\n  stderr:\n{stderr}\n  {}",
            problems.join("\n  ")
        ))
    }
}

#[test]
fn every_case_passes() {
    let mut cases: Vec<PathBuf> = fs::read_dir(cases_dir())
        .expect("cases dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("E2E-"))
        })
        .collect();
    cases.sort();

    assert!(
        cases.len() >= 8,
        "expected the documented FS behaviors to each have a case, found {}",
        cases.len()
    );

    let mut failures = Vec::new();
    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        if let Err(reason) = run_case(case) {
            failures.push(format!("{name}: {reason}"));
        }
    }
    assert!(
        failures.is_empty(),
        "e2e failures:\n{}",
        failures.join("\n\n")
    );
}
