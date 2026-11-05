//! A promoted soft finding says what clears it and how far away that is
//! (§FS-004-check-audit.1.2, §FS-004-check-audit.1.4). The rule does not move: an
//! edit that shrinks a file and leaves it over soft is still refused. Port of the
//! reproducer filed as issue 96: what `fissile init` ships, budgeted at soft 500 and
//! hard 2500, with `src/suite.rs` at 611 lines after 4 over-soft edits.

use std::fs;
use std::path::Path;

use super::base_support::{Work, commit, fissile, folded, json_finding, stdout};
use super::git;

const SUITE: &str = "src/suite.rs";
const SIBLING: &str = "src/other_module.rs";

/// The edit history behind the tip: one version under the soft limit, which is the
/// reset boundary, then four above it.
const HISTORY: [usize; 5] = [450, 600, 605, 608, 611];

/// Write `count` non-blank lines, a blank after every tenth, so the count the rule
/// measures differs from the file's raw length.
fn write_counted(root: &Path, path: &str, count: usize, prefix: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let mut body = String::new();
    for line in 1..=count {
        body.push_str(&format!("fn {prefix}_{line}() {{}}\n"));
        if line % 10 == 0 {
            body.push('\n');
        }
    }
    fs::write(target, body).unwrap();
}

/// A repository holding the config `fissile init` ships, with the source rule moved
/// to the report's budget.
fn shipped(name: &str) -> Work {
    let work = Work::new(name);
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    let init = fissile(root, &["init", ".", "--no-hook"]);
    assert_eq!(init.status.code(), Some(0), "{init:?}");
    let config = root.join(".agent-grounds/fissile.toml");
    let text = fs::read_to_string(&config).unwrap();
    let budgeted = text
        .lines()
        .map(|line| match line {
            "soft = 350" => "soft = 500",
            "hard = 550" => "hard = 2500",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(text.trim_end(), budgeted.trim_end(), "budget not found");
    fs::write(&config, budgeted).unwrap();
    work
}

/// `src/suite.rs` at 611 non-blank lines with 4 prior over-soft edits.
fn tip(name: &str) -> Work {
    let work = shipped(name);
    for lines in HISTORY {
        write_counted(work.path(), SUITE, lines, "t");
        commit(work.path(), &format!("suite at {lines}"));
    }
    work
}

/// Stage one candidate edit: the suite at `kept`, and `moved` lines in a sibling.
fn stage(root: &Path, kept: usize, moved: usize) {
    write_counted(root, SUITE, kept, "t");
    if moved > 0 {
        write_counted(root, SIBLING, moved, "moved");
    }
    git(root, &["add", "-A"]);
}

fn staged(root: &Path) -> std::process::Output {
    fissile(root, &["check", "--staged", "--no-color"])
}

/// The detail line for `path` from a `--no-color` text run, without its indent.
fn detail(text: &str, path: &str) -> String {
    let opening = format!("{path}:");
    text.lines()
        .map(str::trim_start)
        .find(|line| line.starts_with(&opening))
        .unwrap_or_else(|| panic!("no detail for {path} in:\n{text}"))
        .to_owned()
}

/// The reported shrink, the whole line: the number, the unit, and the clause's place
/// between the edit clauses and the ceiling.
#[test]
fn shrinking_to_565_is_refused_and_says_it_is_65_away() {
    let work = tip("clearance-shrink");
    stage(work.path(), 565, 0);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        detail(&stdout(&output), SUITE),
        "src/suite.rs: 565 non-blank lines (budget 500; soft edits 5/5; promoted to blocking; 4 prior committed over-soft edits + 1 staged edit; this staged edit first reaches the promotion limit; to clear, end at or below 500 non-blank lines, 65 away; an exception here would accept 600)"
    );
}

/// The natural seam in the report: 108 lines moved out, 3 left to clear.
#[test]
fn a_seam_that_stops_at_503_is_refused_and_says_it_is_3_away() {
    let work = tip("clearance-seam");
    stage(work.path(), 503, 108);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), SUITE);
    assert!(
        line.contains("; to clear, end at or below 500 non-blank lines, 3 away; "),
        "detail: {line}"
    );
}

#[test]
fn growing_to_617_is_refused_and_says_it_is_117_away() {
    let work = tip("clearance-grow");
    stage(work.path(), 617, 0);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), SUITE);
    assert!(
        line.contains("; to clear, end at or below 500 non-blank lines, 117 away; "),
        "detail: {line}"
    );
}

/// The limit passes, so the distance is `actual - limit` and not one more: one line
/// over says `1 away`, and doing exactly what it says exits 0 with no finding.
#[test]
fn one_line_over_says_1_away_and_the_limit_itself_clears() {
    let work = tip("clearance-edge");
    stage(work.path(), 501, 110);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), SUITE);
    assert!(
        line.contains("; to clear, end at or below 500 non-blank lines, 1 away; "),
        "detail: {line}"
    );

    stage(work.path(), 500, 111);
    let cleared = staged(work.path());
    assert_eq!(cleared.status.code(), Some(0), "{cleared:?}");
    assert_eq!(stdout(&cleared).trim(), "ok");
}

/// The tool-owned epilogue reconciles the number with guidance that says never to
/// cut at the line count, without quoting guidance a project may have rewritten
/// (§FS-004-check-audit.1.2, §DF-003-severity-guidance.1).
#[test]
fn the_epilogue_says_a_smaller_file_is_refused_and_names_both_ways_out() {
    let work = tip("clearance-epilogue");
    stage(work.path(), 565, 0);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let flat = folded(&stdout(&output));
    assert!(
        flat.contains(
            "A file still over its budget is refused even when this edit made it smaller: split so this commit ends it at or below the size its line above names, or, where a seam stops short of that, record the remainder as debt with `fissile exception add <path> --severity soft --rule <rule> --kind <kind>`."
        ),
        "{flat}"
    );
}

/// Record a soft exception at `ceiling` lines over `src/suite.rs` at the tip.
fn tip_with_exception(name: &str, ceiling: &str) -> Work {
    let work = tip(name);
    let added = fissile(
        work.path(),
        &[
            "exception",
            "add",
            SUITE,
            "--severity",
            "soft",
            "--rule",
            "source",
            "--kind",
            "deferred",
            "--reason",
            "The module boundary this suite would split along does not exist yet.",
            "--until",
            "the boundary exists",
            "--max",
            ceiling,
            "--unit",
            "lines",
        ],
    );
    assert_eq!(added.status.code(), Some(0), "{added:?}");
    commit(work.path(), "record the debt");
    work
}

/// `Overflow.limit` is the rule's soft limit even above an exception's ceiling, and
/// the ceiling is what clears: 650 exits 0 with the budget at 500. Naming the budget
/// here would send the author 150 lines past what the gate wants.
#[test]
fn above_a_soft_exception_the_clause_names_its_ceiling_and_not_the_budget() {
    let work = tip_with_exception("clearance-ceiling", "650");
    stage(work.path(), 680, 0);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), SUITE);
    assert!(
        line.contains("(budget 500;")
            && line.contains(
                "; to clear, end at or below the soft exception's 650 non-blank lines, 30 away; "
            ),
        "detail: {line}"
    );

    stage(work.path(), 651, 0);
    let line = detail(&stdout(&staged(work.path())), SUITE);
    assert!(
        line.contains("the soft exception's 650 non-blank lines, 1 away; "),
        "detail: {line}"
    );

    stage(work.path(), 650, 0);
    let cleared = staged(work.path());
    assert_eq!(cleared.status.code(), Some(0), "{cleared:?}");
    assert_eq!(stdout(&cleared).trim(), "ok");
}

/// A base run renders through the same clause, and its JSON stays what it was:
/// `limit` remains the rule's, with no key for the ceiling or the distance.
#[test]
fn a_base_run_above_a_soft_exception_names_its_ceiling() {
    let work = tip_with_exception("clearance-base-ceiling", "650");
    git(work.path(), &["checkout", "-q", "-b", "topic"]);
    write_counted(work.path(), SUITE, 680, "t");
    commit(work.path(), "topic grows past the ceiling");

    let output = fissile(work.path(), &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), SUITE);
    assert!(
        line.contains(
            "; to clear, end at or below the soft exception's 650 non-blank lines, 30 away; "
        ),
        "detail: {line}"
    );

    let json = fissile(
        work.path(),
        &["check", "--base", "main", "--format", "json"],
    );
    assert_eq!(json.status.code(), Some(1), "{json:?}");
    let finding = json_finding(&json, SUITE);
    assert_eq!(finding["limit"], 500);
    assert_eq!(finding["actual"], 680);
}

/// Each file says its own distance, and a file in the same run with grace left
/// says nothing, because it refuses nothing.
#[test]
fn each_promoted_file_says_its_own_distance_and_an_advisory_one_says_none() {
    let work = shipped("clearance-group");
    let root = work.path();
    for lines in HISTORY {
        write_counted(root, SUITE, lines, "t");
        write_counted(root, SIBLING, lines, "o");
        commit(root, &format!("both at {lines}"));
    }
    write_counted(root, SUITE, 565, "t");
    write_counted(root, SIBLING, 503, "o");
    write_counted(root, "src/fresh.rs", 501, "f");
    git(root, &["add", "-A"]);

    let output = staged(root);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(
        detail(&text, SUITE).contains("non-blank lines, 65 away; "),
        "{text}"
    );
    assert!(
        detail(&text, SIBLING).contains("non-blank lines, 3 away; "),
        "{text}"
    );
    let fresh = detail(&text, "src/fresh.rs");
    assert!(fresh.contains("soft edits 1/5"), "{fresh}");
    assert!(!fresh.contains("to clear"), "{fresh}");
}

/// Both files of `UNITS_CONFIG` at their fifth over-soft edit, staged.
fn units_fixture(name: &str) -> Work {
    let work = Work::new(name);
    let root = work.path();
    fs::create_dir_all(root.join(".agent-grounds")).unwrap();
    fs::create_dir_all(root.join("data")).unwrap();
    fs::write(root.join(".agent-grounds/fissile.toml"), UNITS_CONFIG).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    for edit in 0..4 {
        write_counted(root, "notes/a.txt", 30 + edit, "n");
        fs::write(root.join("data/blob.bin"), "x".repeat(200 + edit)).unwrap();
        commit(root, &format!("over-soft edit {}", edit + 1));
    }
    // 21 non-blank lines and 2 blanks: 23 physical, 3 over a soft limit of 20.
    write_counted(root, "notes/a.txt", 21, "n");
    fs::write(root.join("data/blob.bin"), "x".repeat(250)).unwrap();
    git(root, &["add", "-A"]);
    work
}

/// A byte detail has no budget clause, so the clearing clause carries its own number
/// and unit.
#[test]
fn a_byte_rule_says_bytes_with_its_own_number() {
    let work = units_fixture("clearance-bytes");
    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), "data/blob.bin");
    assert!(
        line.starts_with("data/blob.bin: 250 bytes (soft edits 5/5;")
            && line.contains("; to clear, end at or below 100 bytes, 150 away; "),
        "detail: {line}"
    );
}

/// A rule that counts blank lines is cleared in physical lines, not the default basis.
#[test]
fn a_physical_line_rule_says_physical_lines() {
    let work = units_fixture("clearance-physical");
    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output), "notes/a.txt");
    assert!(
        line.starts_with("notes/a.txt: 23 physical lines (budget 20; soft edits 5/5;")
            && line.contains("; to clear, end at or below 20 physical lines, 3 away; "),
        "detail: {line}"
    );
}

const UNITS_CONFIG: &str = r#"
fissile_config_version = 1

[scan]
include = ["data", "notes"]
exclude = []
respect_gitignore = false

[output]
format = "text"
color = "never"
success = "ok"

[[messages]]
id = "split"
text = "Split now or record the debt."

[[rules]]
id = "blobs"
include = ["data/**"]
unit = "bytes"
soft = 100
hard = 100000
message = "split"

[[rules]]
id = "notes"
include = ["notes/**"]
unit = "lines"
soft = 20
hard = 2000
count_blank_lines = true
count_comment_lines = true
message = "split"
"#;

/// Guard, passing today and after: the clause is text only, so a promoted record
/// keeps exactly the keys it has and the same exit (§FS-004-check-audit.1.4).
#[test]
fn a_promoted_json_record_gains_no_key_and_keeps_its_exit() {
    let work = tip("clearance-json");
    stage(work.path(), 565, 0);

    let output = fissile(work.path(), &["check", "--staged", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let finding = json_finding(&output, SUITE);
    let mut keys: Vec<&str> = finding
        .as_object()
        .expect("a finding object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "actual",
            "exception_would_accept",
            "limit",
            "message",
            "message_id",
            "path",
            "promotion",
            "rule_id",
            "severity",
            "soft_edit_count",
            "soft_edit_history_complete",
            "soft_edit_limit",
            "unit",
        ]
    );
    assert_eq!(finding["actual"], 565);
    assert_eq!(finding["limit"], 500);
    assert_eq!(finding["promotion"], "soft_edit_limit");
    assert!(
        !finding["message"].as_str().unwrap().contains("to clear"),
        "{finding}"
    );
}

/// Guard, passing today and after: the hard tier is out of scope, and has its own
/// remedy (§FS-004-check-audit.1.2).
#[test]
fn a_hard_finding_names_nothing_to_clear() {
    let work = tip("clearance-hard");
    stage(work.path(), 2501, 0);

    let output = staged(work.path());
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(text.starts_with("hard:"), "{text}");
    assert!(!text.contains("to clear"), "{text}");
}
