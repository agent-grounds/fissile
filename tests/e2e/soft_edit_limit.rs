//! Real-history command contracts for bounded staged soft debt
//! (§FS-004-check-audit.1.4). These are code-driven rather than a static case:
//! the reset boundary, complete and shallow histories, and staged threshold
//! edit have to be created as Git objects before the real binary runs.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const DEFAULT_LIMIT_CONFIG: &str = r#"
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
text = "The file is over its hard size limit."

[[rules]]
id = "rust"
include = ["src/**/*.rs"]
unit = "lines"
soft = 2
hard = 8
count_blank_lines = false
count_comment_lines = true
soft_message = "split-now"
hard_message = "hard-stop"
"#;

const SOFT_EXCEPTION: &str = r#"
fissile_exceptions_version = 2

[[exceptions]]
path = "src/debt.rs"
match = "exact"
rules = ["rust"]
kind = "deferred"
max_accepted = { value = 8, unit = "lines" }
until = "the history fixture extracts its helper"
reason = "The fixture records this debt to prove that the soft route still silences it."
"#;

struct Work(PathBuf);

impl Work {
    fn new(name: &str) -> Self {
        let scratch = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || {
                let home =
                    std::env::var_os("HOME").expect("HOME names the test scratch filesystem");
                PathBuf::from(home).join("f/tmp")
            },
            PathBuf::from,
        );
        let root = scratch.join(format!(
            "fissile-soft-edit-e2e-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create ~/f/tmp history fixture");
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

fn git<I, S>(root: &Path, args: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git failed: {}",
        output_text(&output)
    );
}

fn initialize(root: &Path, config: &str) {
    fs::create_dir_all(root.join(".agent-grounds")).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join(".agent-grounds/fissile.toml"), config).unwrap();
    git(root, ["init", "-q"]);
}

fn write_lines(root: &Path, count: usize) {
    let content = (1..=count)
        .map(|line| format!("fn line_{line}() {{}}\n"))
        .collect::<String>();
    fs::write(root.join("src/debt.rs"), content).unwrap();
}

fn stage(root: &Path) {
    git(root, ["add", "-A"]);
}

fn commit(root: &Path, message: &str) {
    stage(root);
    git(
        root,
        [
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
            message,
        ],
    );
}

fn status(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn expect(problems: &mut Vec<String>, condition: bool, message: impl Into<String>) {
    if !condition {
        problems.push(message.into());
    }
}

fn expect_run(
    problems: &mut Vec<String>,
    label: &str,
    output: &Output,
    code: i32,
    needles: &[&str],
) {
    let text = stdout(output);
    expect(
        problems,
        status(output) == code,
        format!(
            "{label}: exit {}, expected {code}\n{}",
            status(output),
            output_text(output)
        ),
    );
    for needle in needles {
        expect(
            problems,
            text.contains(needle),
            format!(
                "{label}: stdout missing {needle:?}\n{}",
                output_text(output)
            ),
        );
    }
}

fn message_block<'a>(config: &'a str, id: &str) -> &'a str {
    let marker = format!("id = \"{id}\"");
    let start = config
        .find(&marker)
        .expect("generated message ID is present");
    let rest = &config[start..];
    let end = rest[marker.len()..]
        .find("\n[[")
        .map_or(rest.len(), |offset| marker.len() + offset);
    &rest[..end]
}

fn assert_current_guidance_and_explicit_limits(
    label: &str,
    config: &str,
    source_message: &str,
    document_message: &str,
) {
    let source = message_block(config, source_message);
    let document = message_block(config, document_message);
    let explicit_limits = config.matches("soft_edit_limit = 5").count();
    let soft_rules = config.matches("\nsoft = ").count();
    assert!(
        !source.contains("next time")
            && !document.contains("next time")
            && source.contains("Split now or record the debt now")
            && document.contains("Split now or record the debt now")
            && explicit_limits == soft_rules,
        "{label} source/document guidance must demand the current decision and every soft rule must state the default exactly once; found {explicit_limits} limits for {soft_rules} soft rules"
    );
}

#[test]
fn generated_config_demands_a_current_commit_decision() {
    let work = Work::new("generated-guidance");
    let output = run(&work.0, ["init", ".", "--no-hook"]);
    assert_eq!(status(&output), 0, "init failed: {}", output_text(&output));

    let config = fs::read_to_string(work.0.join(".agent-grounds/fissile.toml"))
        .expect("init writes its config");
    assert_current_guidance_and_explicit_limits(
        "generated config",
        &config,
        "split-source-soft",
        "split-doc-soft",
    );

    let example =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/fissile.toml"))
            .expect("maintained example is readable");
    assert_current_guidance_and_explicit_limits(
        "maintained example",
        &example,
        "split-rust-soft",
        "split-doc-soft",
    );
}

#[test]
fn staged_soft_debt_counts_promotes_resets_and_keeps_its_precedence() {
    let work = Work::new("complete-history");
    initialize(&work.0, DEFAULT_LIMIT_CONFIG);
    write_lines(&work.0, 2);
    commit(&work.0, "at soft limit");

    let mut problems = Vec::new();

    // The staged crossing is edit 1. The omitted version-1 field means five,
    // and text plus JSON expose the counter immediately.
    write_lines(&work.0, 3);
    stage(&work.0);
    let first = run(&work.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "first over-soft edit",
        &first,
        0,
        &["soft: 1 file", "soft edits 1/5"],
    );
    let first_json = run(&work.0, ["check", "--staged", "--format", "json"]);
    expect_run(
        &mut problems,
        "first edit JSON",
        &first_json,
        0,
        &[
            "\"severity\":\"soft\"",
            "\"soft_edit_count\":1",
            "\"soft_edit_limit\":5",
            "\"soft_edit_history_complete\":true",
        ],
    );
    commit(&work.0, "cross soft limit");

    // Commit edits 2 and 3, then leave edit 4 staged. It is the last advisory
    // edit and must still say how much grace remains.
    write_lines(&work.0, 4);
    commit(&work.0, "over soft two");
    write_lines(&work.0, 5);
    commit(&work.0, "over soft three");
    write_lines(&work.0, 6);
    stage(&work.0);
    let fourth = run(&work.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "pre-threshold edit",
        &fourth,
        0,
        &["soft: 1 file", "soft edits 4/5"],
    );
    commit(&work.0, "over soft four");

    // Edit 5 is blocking but remains visibly soft debt and offers the soft
    // registry, never the true hard-size route.
    write_lines(&work.0, 7);
    stage(&work.0);
    let fifth = run(&work.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "threshold edit",
        &fifth,
        1,
        &[
            "soft (promoted): 1 file",
            "soft edits 5/5; promoted to blocking",
            "record the soft-limit debt",
            "--severity soft",
        ],
    );
    let fifth_json = run(&work.0, ["check", "--staged", "--format", "json"]);
    expect_run(
        &mut problems,
        "threshold JSON",
        &fifth_json,
        1,
        &[
            "\"severity\":\"soft\"",
            "\"soft_edit_count\":5",
            "\"soft_edit_limit\":5",
            "\"soft_edit_history_complete\":true",
            "\"promotion\":\"soft_edit_limit\"",
        ],
    );

    // Snapshot surfaces do not turn old debt into a failure or carry staged
    // edit provenance, even while the staged view is at its threshold.
    for (label, args) in [
        ("plain check", vec!["check", "src/debt.rs", "--no-color"]),
        ("audit", vec!["audit", "--no-color"]),
    ] {
        let snapshot = run(&work.0, args);
        expect_run(&mut problems, label, &snapshot, 0, &["soft: 1 file"]);
        expect(
            &mut problems,
            !stdout(&snapshot).contains("soft edits"),
            format!(
                "{label}: snapshot output carried edit provenance\n{}",
                output_text(&snapshot)
            ),
        );
    }

    // A matching soft exception silences the same debt after promotion.
    fs::write(work.0.join("soft-exceptions.toml"), SOFT_EXCEPTION).unwrap();
    stage(&work.0);
    let excepted_fifth = run(&work.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "excepted threshold",
        &excepted_fifth,
        0,
        &["ok"],
    );

    // Commit the over-soft tip, then commit a shrink. The at-limit version is a
    // reset boundary, so the next staged crossing is edit 1 again.
    commit(&work.0, "record threshold debt");
    write_lines(&work.0, 2);
    fs::remove_file(work.0.join("soft-exceptions.toml")).unwrap();
    commit(&work.0, "shrink to reset");
    write_lines(&work.0, 3);
    stage(&work.0);
    let reset = run(&work.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "post-shrink crossing",
        &reset,
        0,
        &["soft: 1 file", "soft edits 1/5"],
    );

    // The soft exception also silences before promotion.
    fs::write(work.0.join("soft-exceptions.toml"), SOFT_EXCEPTION).unwrap();
    stage(&work.0);
    let excepted_first = run(&work.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "excepted first edit",
        &excepted_first,
        0,
        &["ok"],
    );

    // True hard size wins and carries no soft edit provenance.
    fs::remove_file(work.0.join("soft-exceptions.toml")).unwrap();
    write_lines(&work.0, 9);
    stage(&work.0);
    let hard = run(&work.0, ["check", "--staged", "--format", "json"]);
    expect_run(
        &mut problems,
        "hard precedence",
        &hard,
        1,
        &["\"severity\":\"hard\"", "\"limit\":8"],
    );
    let hard_text = stdout(&hard);
    expect(
        &mut problems,
        !hard_text.contains("soft_edit_") && !hard_text.contains("promotion"),
        format!(
            "hard precedence: hard finding carried soft provenance\n{}",
            output_text(&hard)
        ),
    );

    assert!(
        problems.is_empty(),
        "bounded soft-edit contract failures:\n{}",
        problems.join("\n\n")
    );
}

#[test]
fn explicit_limit_and_shallow_history_obey_the_proof_boundary() {
    let mut problems = Vec::new();
    let invalid = Work::new("invalid-limit");
    let zero_limit = DEFAULT_LIMIT_CONFIG.replace(
        "soft = 2\nhard = 8",
        "soft = 2\nsoft_edit_limit = 0\nhard = 8",
    );
    initialize(&invalid.0, &zero_limit);
    write_lines(&invalid.0, 3);
    stage(&invalid.0);
    let invalid_output = run(&invalid.0, ["check", "--staged", "--no-color"]);
    let invalid_error = String::from_utf8_lossy(&invalid_output.stderr);
    expect(
        &mut problems,
        status(&invalid_output) == 2
            && invalid_error.contains("soft_edit_limit")
            && !invalid_error.contains("unknown field"),
        format!(
            "zero must be rejected as an invalid edit limit, not as an unknown setting: {}",
            output_text(&invalid_output)
        ),
    );

    let custom = Work::new("custom-limit");
    let config = DEFAULT_LIMIT_CONFIG.replace(
        "soft = 2\nhard = 8",
        "soft = 2\nsoft_edit_limit = 2\nhard = 8",
    );
    initialize(&custom.0, &config);
    write_lines(&custom.0, 2);
    commit(&custom.0, "at soft limit");

    write_lines(&custom.0, 3);
    stage(&custom.0);
    let first = run(&custom.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "custom pre-threshold",
        &first,
        0,
        &["soft edits 1/2"],
    );
    commit(&custom.0, "cross soft limit");
    write_lines(&custom.0, 4);
    stage(&custom.0);
    let second = run(&custom.0, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "custom threshold",
        &second,
        1,
        &["soft (promoted):", "soft edits 2/2; promoted to blocking"],
    );

    // Build a complete five-edit source, then hide its reset boundary behind a
    // real shallow clone. The clone may report only what it can prove and may
    // not block even if the hidden history would have crossed the threshold.
    let source = Work::new("shallow-source");
    initialize(&source.0, DEFAULT_LIMIT_CONFIG);
    write_lines(&source.0, 2);
    commit(&source.0, "at soft limit");
    for count in 3..=7 {
        write_lines(&source.0, count);
        commit(&source.0, &format!("over soft {}", count - 2));
    }

    let shallow_parent = Work::new("shallow-parent");
    let shallow = shallow_parent.0.join("repo");
    let url = format!("file://{}", source.0.display());
    let clone = Command::new("git")
        .args(["clone", "-q", "--depth", "1", &url])
        .arg(&shallow)
        .output()
        .expect("git clone runs");
    assert!(
        clone.status.success(),
        "shallow clone failed: {}",
        output_text(&clone)
    );
    write_lines(&shallow, 8);
    stage(&shallow);
    let shallow_text = run(&shallow, ["check", "--staged", "--no-color"]);
    expect_run(
        &mut problems,
        "shallow text",
        &shallow_text,
        0,
        &[
            "soft: 1 file",
            "soft edits 1/5",
            "history incomplete; promotion disabled",
        ],
    );
    let shallow_json = run(&shallow, ["check", "--staged", "--format", "json"]);
    expect_run(
        &mut problems,
        "shallow JSON",
        &shallow_json,
        0,
        &[
            "\"soft_edit_count\":1",
            "\"soft_edit_limit\":5",
            "\"soft_edit_history_complete\":false",
        ],
    );
    expect(
        &mut problems,
        !stdout(&shallow_json).contains("\"promotion\""),
        format!(
            "shallow JSON manufactured promotion\n{}",
            output_text(&shallow_json)
        ),
    );

    // A path check outside Git has no history surface at all. It remains a
    // snapshot warning and does not pretend that the file has spent grace.
    let non_git = Work::new("non-git");
    fs::create_dir_all(non_git.0.join(".agent-grounds")).unwrap();
    fs::create_dir_all(non_git.0.join("src")).unwrap();
    fs::write(
        non_git.0.join(".agent-grounds/fissile.toml"),
        DEFAULT_LIMIT_CONFIG,
    )
    .unwrap();
    write_lines(&non_git.0, 7);
    let non_git_check = run(&non_git.0, ["check", "src/debt.rs", "--no-color"]);
    expect_run(
        &mut problems,
        "non-Git path check",
        &non_git_check,
        0,
        &["soft: 1 file"],
    );
    expect(
        &mut problems,
        !stdout(&non_git_check).contains("soft edits"),
        format!(
            "non-Git path check manufactured edit provenance\n{}",
            output_text(&non_git_check)
        ),
    );

    assert!(
        problems.is_empty(),
        "config/history proof-boundary failures:\n{}",
        problems.join("\n\n")
    );
}
