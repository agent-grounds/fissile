//! The repository root, however it is spelled, is a directory: `measure` and
//! `check` report it as `.` and keep measuring the other paths
//! (§FS-004-check-audit.5, §FS-007-measure.1), and the exact-path exception
//! commands refuse it as the repository root (§FS-003-exceptions.3). The
//! absolute spelling depends on where the fixture lives, so these scenarios
//! build their tree at run time rather than under `cases/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CONFIG: &str = r#"fissile_config_version = 1

[scan]
include = ["src"]
exclude = []
respect_gitignore = false

[[messages]]
id = "m"
text = "Split the file."

[[rules]]
id = "rust"
include = ["src/**/*.rs"]
unit = "lines"
soft = 2
hard = 4
message = "m"
"#;

const ROOT_IS_A_DIRECTORY: &str =
    "cannot measure .: is a directory (pass files, or run fissile audit)";

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
            "fissile-repository-root-e2e-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".agent-grounds")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join(".agent-grounds/fissile.toml"), CONFIG).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        let init = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .output()
            .expect("git runs");
        assert!(init.status.success(), "git init failed");
        // Canonical, so the absolute spelling is the one the binary resolves.
        Self(fs::canonicalize(&root).unwrap())
    }

    /// Every way a caller names the root: the spellings issue #76 reported.
    fn root_spellings(&self) -> Vec<String> {
        vec![
            ".".to_owned(),
            "./".to_owned(),
            "src/..".to_owned(),
            self.0.display().to_string(),
        ]
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("fissile runs")
}

fn describe(args: &[&str], output: &Output) -> String {
    format!(
        "fissile {}\n  exit: {:?}\n  stdout:\n{}\n  stderr:\n{}",
        args.join(" "),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn issue_76_every_spelling_of_the_root_is_a_directory_named_dot() {
    let work = Work::new("spellings");
    let mut problems = Vec::new();
    for command in ["measure", "check"] {
        for spelling in work.root_spellings() {
            let args = [command, spelling.as_str(), "--no-color"];
            let output = run(&work.0, &args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() != Some(2) || !stderr.contains(ROOT_IS_A_DIRECTORY) {
                problems.push(describe(&args, &output));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "the root was not reported as `{ROOT_IS_A_DIRECTORY}` with exit 2:\n{}",
        problems.join("\n")
    );
}

#[test]
fn issue_76_the_root_beside_a_file_still_measures_the_file() {
    let work = Work::new("beside-a-file");
    let mut problems = Vec::new();
    for spelling in work.root_spellings() {
        let args = ["measure", spelling.as_str(), "src/main.rs", "--no-color"];
        let output = run(&work.0, &args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A file-level failure, not a run-level one (§FS-004-check-audit.5).
        if output.status.code() != Some(2)
            || !stdout.contains("src/main.rs 1 lines [rust]")
            || !stderr.contains(ROOT_IS_A_DIRECTORY)
        {
            problems.push(describe(&args, &output));
        }
    }
    assert!(
        problems.is_empty(),
        "the root aborted the run instead of being skipped:\n{}",
        problems.join("\n")
    );
}

#[test]
fn issue_76_exact_exception_commands_refuse_the_root() {
    let work = Work::new("exceptions");
    let registry = work.0.join("docs/file-size-agent-exceptions.toml");
    let mut problems = Vec::new();
    for spelling in work.root_spellings() {
        let commands: [&[&str]; 3] = [
            &[
                "exception",
                "add",
                &spelling,
                "--severity",
                "soft",
                "--rule",
                "rust",
                "--kind",
                "structural",
                "--reason",
                "the root is not a file",
            ],
            &[
                "exception",
                "retune",
                &spelling,
                "--severity",
                "soft",
                "--rule",
                "rust",
            ],
            &[
                "exception",
                "remove",
                &spelling,
                "--severity",
                "soft",
                "--rule",
                "rust",
            ],
        ];
        for args in commands {
            let output = run(&work.0, args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if output.status.code() != Some(2) || !stderr.contains("repository root") {
                problems.push(describe(args, &output));
            }
        }
    }
    if registry.exists() {
        problems.push(format!("{} was written", registry.display()));
    }
    assert!(
        problems.is_empty(),
        "an exact exception path naming the root was not refused as the repository root:\n{}",
        problems.join("\n")
    );
}
