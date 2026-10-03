//! An audit whose scan selected no files says so on stderr and leaves stdout
//! as it is (§FS-004-check-audit.2.2). The fixture is the shape the report
//! landed in: a directory the enclosing git repository ignores, so every file
//! under it is filtered out of the scan.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The words that mark the note; the paths it names are asserted separately.
const EMPTY_SCAN: &str = "the scan selected no files";

struct Work(PathBuf);

impl Work {
    fn new(name: &str) -> Self {
        let scratch = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .expect("HOME or USERPROFILE names the test scratch filesystem");
                PathBuf::from(home).join("ag/tmp")
            },
            PathBuf::from,
        );
        let root = scratch.join(format!(
            "fissile-empty-scan-e2e-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create empty-scan fixture");
        // Canonical, so the paths the note prints compare without a symlink in
        // the scratch path standing between them.
        Self(fs::canonicalize(&root).expect("canonicalize empty-scan fixture"))
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn audit(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(dir)
        .arg("audit")
        .args(args)
        .arg("--no-color")
        .output()
        .expect("fissile runs")
}

fn git_ok(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        text(&output)
    );
}

/// A repository holding three 200-line files under `<dir>/src`, committed.
fn repository(root: &Path, dir: &str, ignore: Option<&str>) {
    let src = root.join(dir).join("src");
    fs::create_dir_all(&src).unwrap();
    for n in 1..=3 {
        let body: String = (0..200).map(|i| format!("fn f{n}_{i}() {{}}\n")).collect();
        fs::write(src.join(format!("big{n}.rs")), body).unwrap();
    }
    if let Some(ignore) = ignore {
        fs::write(root.join(".gitignore"), ignore).unwrap();
    }
    git_ok(root, &["init", "-q", "-b", "main"]);
    git_ok(root, &["config", "user.email", "e2e@fissile.invalid"]);
    git_ok(root, &["config", "user.name", "e2e"]);
    git_ok(root, &["add", "-A"]);
    git_ok(
        root,
        &[
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-q",
            "-m",
            "initial",
        ],
    );
}

fn text(output: &Output) -> String {
    format!(
        "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The note names the empty selection, the directory and the repository root,
/// and nothing about stdout or the exit status moves (§FS-004-check-audit.2.2).
#[test]
fn issue_77_audit_over_an_ignored_directory_says_it_measured_nothing() {
    let work = Work::new("ignored");
    let outer = work.0.join("outer");
    repository(&outer, "proj", Some("proj/\n"));
    let proj = outer.join("proj");
    let dir_clause = format!("in {}", proj.display());
    let root_clause = format!("inside the git repository at {}", outer.display());

    let runs: [(&[&str], Option<&str>); 3] = [
        (&["--top", "3"], Some("ok\n")),
        (&["--top", "20", "--only", "top"], Some("")),
        (&["--format", "json"], None),
    ];
    for (args, expected_stdout) in runs {
        let output = audit(&proj, args);
        let report = format!(
            "fissile audit {args:?} in {}\n{}",
            proj.display(),
            text(&output)
        );
        assert_eq!(output.status.code(), Some(0), "{report}");
        let err = stderr(&output);
        assert!(err.contains(EMPTY_SCAN), "no empty-scan note: {report}");
        assert!(
            err.contains(&dir_clause),
            "note does not name {dir_clause:?}: {report}"
        );
        assert!(
            err.contains(&root_clause),
            "note does not name {root_clause:?}: {report}"
        );
        match expected_stdout {
            Some(expected) => {
                assert_eq!(
                    String::from_utf8_lossy(&output.stdout),
                    expected,
                    "{report}"
                );
            }
            None => {
                let json: serde_json::Value =
                    serde_json::from_slice(&output.stdout).expect("stdout is one JSON document");
                assert!(json.get("findings").is_some(), "JSON shape moved: {report}");
            }
        }
    }
}

/// The control: the same files in a repository that selects them print the
/// ranking and no note (§FS-004-check-audit.2.2).
#[test]
fn issue_77_audit_over_a_selected_tree_prints_no_empty_scan_note() {
    let work = Work::new("selected");
    let plain = work.0.join("plain");
    repository(&plain, ".", None);

    let output = audit(&plain, &["--top", "3"]);
    let report = text(&output);
    assert_eq!(output.status.code(), Some(0), "{report}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("src/big1.rs"),
        "{report}"
    );
    assert!(
        !stderr(&output).contains(EMPTY_SCAN),
        "note over a non-empty scan: {report}"
    );
}
