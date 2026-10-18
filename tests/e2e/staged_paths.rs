//! Git display quoting must not change literal staged selection
//! (§FS-004-check-audit.1.6), including the selection shared by
//! `measure --staged` (§FS-007-measure.1).

use std::fs;
use std::path::PathBuf;
use std::process::Output;

use super::base_support::{CONFIG, fissile, write_lines};
use super::git;

const PATH: &str = "docs/café.md";
const EXCLUDED: &str = "docs/generated/cafés.md";

struct Work(PathBuf);

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn staged(name: &str, indexed_lines: usize, working_lines: usize) -> Work {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .expect("HOME or USERPROFILE names the scratch filesystem");
    let work = Work(PathBuf::from(home).join(format!(
        "ag/tmp/fissile-94-e2e-{}-{name}",
        std::process::id()
    )));
    fs::create_dir_all(work.0.join(".agent-grounds")).unwrap();
    fs::write(work.0.join(".agent-grounds/fissile.toml"), CONFIG).unwrap();
    git(&work.0, &["init", "-q"]);
    git(&work.0, &["config", "core.quotePath", "true"]);
    write_lines(&work.0, PATH, indexed_lines, "indexed");
    write_lines(&work.0, EXCLUDED, 202, "excluded");
    git(&work.0, &["add", "--", PATH, EXCLUDED]);
    write_lines(&work.0, PATH, working_lines, "unstaged");
    work
}

fn diagnostic(output: &Output) -> String {
    format!(
        "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn issue_94_staged_check_accepts_under_budget_utf8_path() {
    // An over-hard working copy and excluded file must not block the one-line index.
    let work = staged("under-budget", 1, 201);
    let output = fissile(&work.0, &["check", "--staged", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{}", diagnostic(&output));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "ok\n");
    assert!(output.stderr.is_empty(), "{}", diagnostic(&output));
}

#[test]
fn issue_94_staged_check_blocks_over_budget_utf8_path() {
    // Selecting nothing, reading the working copy, or losing exclusions cannot pass.
    let work = staged("over-budget", 201, 1);
    let output = fissile(&work.0, &["check", "--staged", "--no-color"]);
    assert_eq!(output.status.code(), Some(1), "{}", diagnostic(&output));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("hard: 1 file"), "{}", diagnostic(&output));
    assert!(
        text.contains(&format!("{PATH}: 201 non-blank lines (budget 200;")),
        "{}",
        diagnostic(&output)
    );
    assert!(!text.contains(EXCLUDED), "{}", diagnostic(&output));
    assert!(output.stderr.is_empty(), "{}", diagnostic(&output));
}

#[test]
fn issue_94_staged_measure_reports_literal_utf8_path_and_indexed_count() {
    let work = staged("measure", 3, 1);
    let output = fissile(&work.0, &["measure", "--staged", "--format", "json"]);
    assert_eq!(output.status.code(), Some(0), "{}", diagnostic(&output));
    let records: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let records = records.as_array().expect("measurement records");
    assert_eq!(records.len(), 1, "{records:?}");
    assert_eq!(records[0]["path"], PATH);
    assert_eq!(records[0]["actual"], 3);
    assert_eq!(records[0]["unit"], "lines");
    assert_eq!(records[0]["rule_id"], "citable-spec");
    assert!(output.stderr.is_empty(), "{}", diagnostic(&output));
}
