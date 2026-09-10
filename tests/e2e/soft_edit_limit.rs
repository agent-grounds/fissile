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

fn write_named_lines(root: &Path, relative: &str, count: usize, name: &str) {
    let content = (1..=count)
        .map(|line| format!("fn {name}_{line}() {{}}\n"))
        .collect::<String>();
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture path has a parent")).unwrap();
    fs::write(path, content).unwrap();
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

fn assert_promoted_record_matches_closed_schema(output: &str) {
    let records: serde_json::Value = serde_json::from_str(output).expect("finding JSON parses");
    let record = records
        .as_array()
        .and_then(|records| records.first())
        .and_then(serde_json::Value::as_object)
        .expect("one finding object");
    let schema_text = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/finding.schema.json"),
    )
    .unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema_text).expect("schema JSON parses");
    assert_eq!(schema["additionalProperties"], false);
    let properties = schema["properties"].as_object().expect("schema properties");
    for key in record.keys() {
        assert!(
            properties.contains_key(key),
            "closed finding schema rejects emitted key {key}"
        );
    }
    for key in [
        "soft_edit_count",
        "soft_edit_limit",
        "soft_edit_history_complete",
        "promotion",
    ] {
        assert!(record.contains_key(key), "promoted record misses {key}");
        assert!(properties.contains_key(key), "schema misses {key}");
    }
    assert_eq!(properties["soft_edit_count"]["type"], "integer");
    assert_eq!(properties["soft_edit_count"]["minimum"], 1);
    assert_eq!(properties["soft_edit_limit"]["type"], "integer");
    assert_eq!(properties["soft_edit_limit"]["minimum"], 1);
    assert_eq!(properties["soft_edit_history_complete"]["type"], "boolean");
    assert_eq!(properties["promotion"]["const"], "soft_edit_limit");
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
    assert_promoted_record_matches_closed_schema(&stdout(&fifth_json));

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

#[test]
fn rename_into_scope_starts_the_governed_count_at_one() {
    let work = Work::new("rename-into-scope");
    let config = DEFAULT_LIMIT_CONFIG.replace(
        "soft = 2\nhard = 8",
        "soft = 2\nsoft_edit_limit = 3\nhard = 8",
    );
    initialize(&work.0, &config);
    write_named_lines(&work.0, "notes/debt.txt", 4, "outside1");
    commit(&work.0, "outside rule one");
    for edit in 2..=4 {
        write_named_lines(&work.0, "notes/debt.txt", 4, &format!("outside{edit}"));
        commit(&work.0, &format!("outside rule {edit}"));
    }
    git(&work.0, ["mv", "notes/debt.txt", "src/debt.rs"]);

    let renamed = run(&work.0, ["check", "--staged", "--no-color"]);
    assert_eq!(status(&renamed), 0, "{}", output_text(&renamed));
    assert!(
        stdout(&renamed).contains("soft edits 1/3"),
        "{}",
        output_text(&renamed)
    );
}

#[test]
fn merged_branch_edits_count_without_counting_the_importing_merge() {
    let work = Work::new("merged-branch");
    let config = DEFAULT_LIMIT_CONFIG.replace(
        "soft = 2\nhard = 8",
        "soft = 2\nsoft_edit_limit = 4\nhard = 8",
    );
    initialize(&work.0, &config);
    write_lines(&work.0, 2);
    commit(&work.0, "at soft limit");
    git(&work.0, ["switch", "-qc", "side"]);
    for count in 3..=5 {
        write_lines(&work.0, count);
        commit(&work.0, &format!("side edit {}", count - 2));
    }
    git(&work.0, ["switch", "-q", "main"]);
    fs::write(work.0.join("unrelated.txt"), "main\n").unwrap();
    commit(&work.0, "unrelated main edit");
    git(
        &work.0,
        ["merge", "-q", "--no-ff", "side", "-m", "merge side"],
    );
    write_lines(&work.0, 6);
    stage(&work.0);

    let merged = run(&work.0, ["check", "--staged", "--no-color"]);
    assert_eq!(status(&merged), 1, "{}", output_text(&merged));
    assert!(
        stdout(&merged).contains("soft edits 4/4; promoted to blocking"),
        "{}",
        output_text(&merged)
    );
}

#[test]
fn a_merge_resolution_distinct_from_both_parents_counts_once() {
    let work = Work::new("merge-resolution");
    let config = DEFAULT_LIMIT_CONFIG.replace(
        "soft = 2\nhard = 8",
        "soft = 10\nsoft_edit_limit = 4\nhard = 20",
    );
    initialize(&work.0, &config);
    let base = (1..=10)
        .map(|line| format!("fn base_{line}() {{}}\n"))
        .collect::<String>();
    fs::write(work.0.join("src/debt.rs"), &base).unwrap();
    commit(&work.0, "at soft limit");

    git(&work.0, ["switch", "-qc", "side"]);
    let side = base.replace("fn base_2() {}\n", "fn base_2() {}\nfn side() {}\n");
    fs::write(work.0.join("src/debt.rs"), side).unwrap();
    commit(&work.0, "side over soft");

    git(&work.0, ["switch", "-q", "main"]);
    let main = base.replace("fn base_8() {}\n", "fn base_8() {}\nfn main_edit() {}\n");
    fs::write(work.0.join("src/debt.rs"), main).unwrap();
    commit(&work.0, "main over soft");
    git(
        &work.0,
        ["merge", "-q", "--no-ff", "side", "-m", "resolve both edits"],
    );
    let mut staged = fs::read_to_string(work.0.join("src/debt.rs")).unwrap();
    staged.push_str("fn staged() {}\n");
    fs::write(work.0.join("src/debt.rs"), staged).unwrap();
    stage(&work.0);

    let merged = run(&work.0, ["check", "--staged", "--no-color"]);
    assert_eq!(status(&merged), 1, "{}", output_text(&merged));
    assert!(
        stdout(&merged).contains("soft edits 4/4; promoted to blocking"),
        "{}",
        output_text(&merged)
    );
}

#[test]
fn parallel_reset_frontiers_close_independently() {
    let work = Work::new("parallel-reset");
    let config = DEFAULT_LIMIT_CONFIG.replace(
        "soft = 2\nhard = 8",
        "soft = 10\nsoft_edit_limit = 2\nhard = 20",
    );
    initialize(&work.0, &config);
    let base = (1..=10)
        .map(|line| format!("fn base_{line}() {{}}\n"))
        .collect::<String>();
    fs::write(work.0.join("src/debt.rs"), &base).unwrap();
    commit(&work.0, "at soft limit");

    git(&work.0, ["switch", "-qc", "side"]);
    let side = base.replace("fn base_2() {}", "fn side_reset() {}");
    fs::write(work.0.join("src/debt.rs"), side).unwrap();
    commit(&work.0, "side reset at soft");

    git(&work.0, ["switch", "-q", "main"]);
    let main = base.replace("fn base_8() {}", "fn main_reset() {}");
    fs::write(work.0.join("src/debt.rs"), main).unwrap();
    commit(&work.0, "main resets at soft");
    git(&work.0, ["merge", "-q", "--no-ff", "--no-commit", "side"]);
    let mut merged = fs::read_to_string(work.0.join("src/debt.rs")).unwrap();
    merged.push_str("fn merge_crossing() {}\n");
    fs::write(work.0.join("src/debt.rs"), merged).unwrap();
    commit(&work.0, "merge crosses soft");
    let mut staged = fs::read_to_string(work.0.join("src/debt.rs")).unwrap();
    staged.push_str("fn staged() {}\n");
    fs::write(work.0.join("src/debt.rs"), staged).unwrap();
    stage(&work.0);

    let checked = run(&work.0, ["check", "--staged", "--no-color"]);
    assert_eq!(status(&checked), 1, "{}", output_text(&checked));
    assert!(
        stdout(&checked).contains("soft edits 2/2; promoted to blocking"),
        "{}",
        output_text(&checked)
    );
}

#[test]
fn duplicate_rule_ids_keep_declaration_edit_limits() {
    let work = Work::new("duplicate-rule-edit-limits");
    let config = r#"
fissile_config_version = 1

[[messages]]
id = "split-now"
text = "Split now."

[[rules]]
id = "duplicate"
include = ["src/**/*.rs"]
unit = "bytes"
soft = 2
soft_edit_limit = 2
hard = 1000
message = "split-now"

[[rules]]
id = "duplicate"
include = ["src/**/*.rs"]
unit = "lines"
soft = 2
soft_edit_limit = 7
hard = 8
message = "split-now"
"#;
    initialize(&work.0, config);
    write_lines(&work.0, 3);
    commit(&work.0, "both rules over soft");
    write_lines(&work.0, 4);
    stage(&work.0);

    let checked = run(&work.0, ["check", "--staged", "--no-color"]);
    assert_eq!(status(&checked), 1, "{}", output_text(&checked));
    let checked_text = stdout(&checked);
    assert!(checked_text.contains("soft edits 2/2; promoted to blocking"));
    assert!(checked_text.contains("soft edits 2/7"));

    let limits = run(&work.0, ["limits", "--format", "json"]);
    assert_eq!(status(&limits), 0, "{}", output_text(&limits));
    let limits_text = stdout(&limits);
    assert!(
        limits_text.contains(r#""unit":"bytes","soft":2,"soft_edit_limit":2"#),
        "{limits_text}"
    );
    assert!(
        limits_text.contains(r#""unit":"lines","soft":2,"soft_edit_limit":7"#),
        "{limits_text}"
    );
}

#[cfg(unix)]
#[test]
fn token_history_stops_measuring_at_the_recent_reset() {
    use std::os::unix::fs::PermissionsExt;

    let work = Work::new("bounded-token-work");
    let counter = work.0.join("count-token-runs.sh");
    let count_file = work.0.join("token-runs");
    let script = format!(
        "#!/bin/sh\nprintf 'run\\n' >> '{}'\nexec /usr/bin/wc -w \"$1\"\n",
        count_file.display()
    );
    fs::create_dir_all(&work.0).unwrap();
    fs::write(&counter, script).unwrap();
    fs::set_permissions(&counter, fs::Permissions::from_mode(0o755)).unwrap();
    let config = DEFAULT_LIMIT_CONFIG
        .replace("unit = \"lines\"", "unit = \"tokens\"")
        .replace(
            "count_blank_lines = false\ncount_comment_lines = true\n",
            "",
        )
        .replace("hard = 8", "hard = 50")
        .replace(
            "[[messages]]\nid = \"split-now\"",
            &format!(
                "[tokens]\nenabled = true\ncommand = [\"{}\", \"{{path}}\"]\n\n[[messages]]\nid = \"split-now\"",
                counter.display()
            ),
        );
    initialize(&work.0, &config);
    for edit in 1..=10 {
        fs::write(
            work.0.join("src/debt.rs"),
            format!("old over soft version {edit}\n"),
        )
        .unwrap();
        commit(&work.0, &format!("old over-soft {edit}"));
    }
    fs::write(work.0.join("src/debt.rs"), "reset\n").unwrap();
    commit(&work.0, "recent reset");
    fs::write(
        work.0.join("src/debt.rs"),
        "recent crossing version eleven\n",
    )
    .unwrap();
    commit(&work.0, "recent crossing");
    fs::write(
        work.0.join("src/debt.rs"),
        "staged version twelve remains over\n",
    )
    .unwrap();
    stage(&work.0);
    let _ = fs::remove_file(&count_file);

    let checked = run(&work.0, ["check", "--staged", "--no-color"]);
    assert_eq!(status(&checked), 0, "{}", output_text(&checked));
    assert!(stdout(&checked).contains("soft edits 2/5"));
    let invocations = fs::read_to_string(&count_file)
        .expect("token counter ran")
        .lines()
        .count();
    assert_eq!(
        invocations, 3,
        "only staged, crossing, and reset versions are measured"
    );
}
