//! Invocation-boundary regressions for staged promotion
//! (§FS-004-check-audit.1.2, §FS-004-check-audit.1.4). Real Git history and a
//! real commit hook prove that an index-backed staged verdict does not describe
//! the verdict of a hook that passes working-tree paths.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

const CONFIG: &str = r#"
fissile_config_version = 1

[scan]
include = ["src"]
exclude = []
respect_gitignore = false

[output]
format = "text"
color = "never"
success = "ok"

[exceptions]
soft_registry = "soft-exceptions.toml"
hard_registry = "hard-exceptions.toml"
stale = "ignore"

[[messages]]
id = "split-now"
text = "Split now or record the debt now with `fissile exception add --severity soft`."

[[messages]]
id = "hard-stop"
text = "Split before committing."

[[rules]]
id = "core-source"
include = ["src/**/*.rs"]
unit = "lines"
soft = 350
soft_edit_limit = 5
hard = 1000
count_blank_lines = false
count_comment_lines = true
soft_message = "split-now"
hard_message = "hard-stop"
"#;

#[cfg(unix)]
const PROMOTED_EPILOGUE: &str = "\
`fissile check --staged` rejected this staged snapshot. Soft-edit promotion
blocks a commit only when the hook runs that command; a hook that calls
`fissile check <paths>` keeps soft findings advisory. Split now or record the
debt with `fissile exception add <path> --severity soft --rule <rule> --kind
<kind>`. Bypassing such a hook with `--no-verify` leaves the overflow for
review or CI.";

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
            "fissile-staged-invocation-e2e-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create staged-invocation fixture");
        Self(root)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn output_text(output: &Output) -> String {
    format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn run<I, S>(root: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("fissile runs")
}

fn git<I, S>(root: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs")
}

fn git_ok<I, S>(root: &Path, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = git(root, args);
    assert!(
        output.status.success(),
        "git failed: {}",
        output_text(&output)
    );
}

fn initialize(root: &Path) {
    fs::create_dir_all(root.join(".agent-grounds")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join(".agent-grounds/fissile.toml"), CONFIG).unwrap();
    git_ok(root, ["init", "-q", "-b", "main"]);
    git_ok(root, ["config", "user.email", "e2e@fissile.invalid"]);
    git_ok(root, ["config", "user.name", "e2e"]);
}

fn write_lines(root: &Path, relative: &str, count: usize) {
    let stem = relative.replace(['/', '.'], "_");
    let content = (1..=count)
        .map(|line| format!("fn {stem}_{line}() {{}}\n"))
        .collect::<String>();
    fs::write(root.join(relative), content).unwrap();
}

#[cfg(unix)]
fn append_edit(root: &Path, relative: &str, edit: usize) {
    let mut content = fs::read_to_string(root.join(relative)).unwrap();
    content.push_str(&format!("fn edit_{edit}() {{}}\n"));
    fs::write(root.join(relative), content).unwrap();
}

fn stage(root: &Path) {
    git_ok(root, ["add", "-A"]);
}

fn commit_without_hooks(root: &Path, message: &str) {
    stage(root);
    git_ok(
        root,
        [
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=e2e-no-hooks",
            "commit",
            "-q",
            "-m",
            message,
        ],
    );
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[cfg(unix)]
fn check(problems: &mut Vec<String>, condition: bool, message: impl Into<String>) {
    if !condition {
        problems.push(message.into());
    }
}

#[test]
#[cfg(unix)]
fn issue_74_staged_promotion_explains_filename_passing_hook_and_commits() {
    let work = Work::new("filename-hook");
    initialize(&work.0);
    for file in ["src/body.rs", "src/members.rs"] {
        write_lines(&work.0, file, 350);
    }
    commit_without_hooks(&work.0, "at soft limit");

    let mut problems = Vec::new();
    for edit in 1..=4 {
        for file in ["src/body.rs", "src/members.rs"] {
            append_edit(&work.0, file, edit);
        }
        stage(&work.0);
        let staged = run(&work.0, ["check", "--staged", "--no-color"]);
        let text = stdout(&staged);
        check(
            &mut problems,
            code(&staged) == 0 && text.contains(&format!("soft edits {edit}/5")),
            format!(
                "staged edit {edit} should remain advisory\n{}",
                output_text(&staged)
            ),
        );
        commit_without_hooks(&work.0, &format!("over-soft edit {edit}"));
    }

    for file in ["src/body.rs", "src/members.rs"] {
        append_edit(&work.0, file, 5);
    }
    stage(&work.0);

    for file in ["src/body.rs", "src/members.rs"] {
        let indexed = git(&work.0, ["show", &format!(":{file}")]);
        check(
            &mut problems,
            indexed.status.success() && indexed.stdout == fs::read(work.0.join(file)).unwrap(),
            format!("{file}: index and working-tree bytes should be identical"),
        );
    }

    let staged = run(&work.0, ["check", "--staged", "--no-color"]);
    let staged_text = stdout(&staged);
    check(
        &mut problems,
        code(&staged) == 1
            && staged_text.contains("soft edits 5/5; promoted to blocking")
            && staged_text.contains(PROMOTED_EPILOGUE),
        format!(
            "threshold staged check should explain its own promoted verdict\n{}",
            output_text(&staged)
        ),
    );
    check(
        &mut problems,
        !staged_text.contains("commit blocked by fissile"),
        format!("staged check claimed an unknown commit was blocked\n{staged_text}"),
    );

    let explicit = run(
        &work.0,
        ["check", "src/members.rs", "src/body.rs", "--no-color"],
    );
    let explicit_text = stdout(&explicit);
    check(
        &mut problems,
        code(&explicit) == 0
            && explicit_text.contains("soft: 2 files")
            && !explicit_text.contains("soft edits"),
        format!(
            "explicit-path snapshot should remain advisory and history-free\n{}",
            output_text(&explicit)
        ),
    );

    // Keep the hook observation independent of the wording checks above: all
    // failures accumulate until after a real `git commit` has run the snapshot
    // command with the two filenames a hook manager would inject.
    let hook = work.0.join(".git/hooks/pre-commit");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' 'fissile check src/body.rs src/members.rs' > .git/filename-passing-hook.log\nexec '{}' check src/body.rs src/members.rs --no-color\n",
        env!("CARGO_BIN_EXE_fissile")
    );
    fs::write(&hook, script).unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let committed = git(
        &work.0,
        ["-c", "commit.gpgsign=false", "commit", "-m", "fifth edit"],
    );
    check(
        &mut problems,
        committed.status.success(),
        format!(
            "filename-passing hook should accept the commit\n{}",
            output_text(&committed)
        ),
    );
    check(
        &mut problems,
        fs::read_to_string(work.0.join(".git/filename-passing-hook.log"))
            .is_ok_and(|line| line.trim() == "fissile check src/body.rs src/members.rs"),
        "the actual commit did not leave filename-passing hook evidence",
    );

    assert!(
        problems.is_empty(),
        "staged/path invocation contract failures:\n{}",
        problems.join("\n\n")
    );
}

#[test]
fn issue_74_staged_and_path_checks_measure_index_and_working_tree_respectively() {
    let work = Work::new("divergent-bytes");
    initialize(&work.0);
    write_lines(&work.0, "src/body.rs", 350);
    commit_without_hooks(&work.0, "at soft limit");

    write_lines(&work.0, "src/body.rs", 351);
    stage(&work.0);
    write_lines(&work.0, "src/body.rs", 1001);

    let staged = run(&work.0, ["check", "--staged", "--no-color"]);
    let explicit = run(&work.0, ["check", "src/body.rs", "--no-color"]);
    assert_eq!(code(&staged), 0, "{}", output_text(&staged));
    assert!(stdout(&staged).contains("351 non-blank lines"));
    assert_eq!(code(&explicit), 1, "{}", output_text(&explicit));
    assert!(stdout(&explicit).contains("1001 non-blank lines"));
}
