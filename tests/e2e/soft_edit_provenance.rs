//! Staged edit-count explanations with real Git history (§FS-004-check-audit.1.4).
//! Port of the issue 79 intake fixture: a document born over its soft budget,
//! repeatedly committed, then touched by a two-line staged edit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::git;

const CONFIG: &str = r#"
fissile_config_version = 1

[scan]
include = ["docs"]
exclude = [".git/**"]
respect_gitignore = true

[output]
format = "text"
color = "never"
success = "ok"

[[messages]]
id = "split-spec"
text = "Split now or record the soft debt."

[[rules]]
id = "citable-spec"
include = ["docs/**/*.md"]
unit = "lines"
soft = 20
hard = 200
count_blank_lines = false
count_comment_lines = true
message = "split-spec"
"#;

struct Work(PathBuf);

impl Work {
    fn new(name: &str) -> Self {
        let scratch = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .expect("HOME or USERPROFILE names the scratch filesystem");
                PathBuf::from(home).join("ag/tmp")
            },
            PathBuf::from,
        );
        let root = scratch.join(format!(
            "fissile-soft-provenance-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create provenance fixture");
        Self(root)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(
        root,
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
            "-qm",
            message,
        ],
    );
}

fn append(root: &Path, text: &str) {
    let path = root.join("docs/spec.md");
    let mut content = fs::read_to_string(&path).unwrap();
    content.push_str(text);
    fs::write(path, content).unwrap();
}

fn history(name: &str, prior_edits: usize, limit: usize) -> Work {
    let work = Work::new(name);
    fs::create_dir_all(work.0.join(".agent-grounds")).unwrap();
    fs::create_dir_all(work.0.join("docs")).unwrap();
    let config = if limit == 5 {
        CONFIG.to_owned() // Exercise the default edit limit, as in the intake fixture.
    } else {
        CONFIG.replace(
            "soft = 20",
            &format!("soft = 20\nsoft_edit_limit = {limit}"),
        )
    };
    fs::write(work.0.join(".agent-grounds/fissile.toml"), config).unwrap();
    git(&work.0, &["init", "-q"]);
    commit(&work.0, "config; spec absent");
    let content = format!(
        "# Spec\n\n{}",
        (0..60)
            .map(|i| format!("Sentence {i} of the specification.\n"))
            .collect::<String>()
    );
    fs::write(work.0.join("docs/spec.md"), content).unwrap();
    for edit in 0..prior_edits {
        if edit > 0 {
            append(&work.0, &format!("Later addition {edit}.\n"));
        }
        commit(&work.0, &format!("over-soft edit {}", edit + 1));
    }
    work
}

fn stage_two_lines(root: &Path) {
    append(
        root,
        "A two-line edit, line one.\nA two-line edit, line two.\n",
    );
    git(root, &["add", "docs/spec.md"]);
}

fn run(root: &Path, json: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fissile"));
    command
        .current_dir(root)
        .args(["check", "--staged", "--no-color"]);
    if json {
        command.args(["--format", "json"]);
    }
    command.output().expect("fissile runs")
}

fn assert_detail(root: &Path, count: usize, limit: usize, complete: bool, clause: &str) {
    let code = i32::from(complete && count >= limit);
    let json = run(root, true);
    assert_eq!(json.status.code(), Some(code), "{json:?}");
    let records: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(records.as_array().unwrap().len(), 1);
    let finding = &records[0];
    assert_eq!(finding["severity"], "soft");
    assert_eq!(finding["soft_edit_count"], count);
    assert_eq!(finding["soft_edit_limit"], limit);
    assert_eq!(finding["soft_edit_history_complete"], complete);
    if code == 1 {
        assert_eq!(finding["promotion"], "soft_edit_limit");
    } else {
        assert!(finding.get("promotion").is_none());
    }

    let output = run(root, false);
    assert_eq!(output.status.code(), Some(code), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let detail = stdout
        .lines()
        .find(|line| line.contains("docs/spec.md:"))
        .unwrap();
    if !complete {
        for unproved in [
            "starts the over-soft run",
            "first reaches",
            "already reached",
        ] {
            assert!(!detail.contains(unproved), "unproved boundary: {detail}");
        }
    }
    assert!(
        detail.contains(clause),
        "missing staged provenance clause {clause:?}\nactual detail: {detail}"
    );
}

#[test]
fn inherited_ninth_edit_explains_previously_reached_limit() {
    let work = history("inherited-nine", 8, 5);
    stage_two_lines(&work.0);
    assert_detail(
        &work.0,
        9,
        5,
        true,
        "soft edits 9/5; promoted to blocking; 8 prior committed over-soft edits + 1 staged edit; prior committed edits already reached the promotion limit",
    );
}

#[test]
fn fifth_edit_explains_first_promotion() {
    let work = history("first-promotion-five", 4, 5);
    stage_two_lines(&work.0);
    assert_detail(
        &work.0,
        5,
        5,
        true,
        "soft edits 5/5; promoted to blocking; 4 prior committed over-soft edits + 1 staged edit; this staged edit first reaches the promotion limit",
    );
}

#[test]
fn first_edit_explains_start_of_run_without_promotion() {
    let work = history("first-advisory", 0, 5);
    stage_two_lines(&work.0);
    assert_detail(
        &work.0,
        1,
        5,
        true,
        "soft edits 1/5; 0 prior committed over-soft edits + 1 staged edit; this staged edit starts the over-soft run",
    );
}

#[test]
fn limit_one_explains_start_and_first_promotion_together() {
    let work = history("first-promoted", 0, 1);
    stage_two_lines(&work.0);
    assert_detail(
        &work.0,
        1,
        1,
        true,
        "soft edits 1/1; promoted to blocking; 0 prior committed over-soft edits + 1 staged edit; this staged edit starts the over-soft run; this staged edit first reaches the promotion limit",
    );
}

fn assert_shallow_detail(name: &str, depth: usize) {
    let source = history(&format!("{name}-source"), 8, 5);
    let work = Work::new(name);
    let repo = work.0.join("repo");
    let url = format!("file://{}", source.0.display());
    let depth_text = depth.to_string();
    git(
        &work.0,
        &["clone", "-q", "--depth", &depth_text, &url, "repo"],
    );
    stage_two_lines(&repo);
    assert_detail(
        &repo,
        depth,
        5,
        false,
        &format!(
            "soft edits {depth}/5; history incomplete; promotion disabled; at least {} prior committed over-soft edits established + 1 staged edit",
            depth - 1
        ),
    );
}

#[test]
fn shallow_count_one_does_not_claim_a_proved_start() {
    assert_shallow_detail("shallow-one", 1);
}

#[test]
fn shallow_count_above_limit_reports_only_established_evidence() {
    assert_shallow_detail("shallow-six", 6);
}
