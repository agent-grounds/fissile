//! `check --base <ref>` spends soft-edit grace over committed edits, split at the
//! base (§FS-004-check-audit.1.4), and closes a failing run with a base epilogue
//! (§FS-004-check-audit.1.2). Every case here fails today: `--base` is an unknown
//! option, exit 2.

use std::fs;

use super::base_support::{
    Work, branch_after, edit_spec, fissile, folded, init, json_finding, stderr, stdout, write_lines,
};
use super::git;

const LEAD: &str = "`fissile check --base main` rejected the selected HEAD files.";

/// The text detail line for `docs/spec.md`, from a `--no-color` text run.
fn detail(text: &str) -> String {
    text.lines()
        .find(|line| line.contains("docs/spec.md:"))
        .unwrap_or_else(|| panic!("no detail for docs/spec.md in:\n{text}"))
        .to_owned()
}

#[test]
fn four_inherited_and_two_branch_edits_block_with_the_split() {
    let work = branch_after("four-plus-two", 4);
    edit_spec(work.path(), 40, "topic edit 1");
    edit_spec(work.path(), 41, "topic edit 2");

    // Neither question the issue started from answers it: nothing is staged, and
    // a snapshot never promotes.
    let staged = fissile(work.path(), &["check", "--staged", "--no-color"]);
    assert_eq!(staged.status.code(), Some(0), "{staged:?}");
    let snapshot = fissile(work.path(), &["check", "docs/spec.md", "--no-color"]);
    assert_eq!(snapshot.status.code(), Some(0), "{snapshot:?}");

    let output = fissile(work.path(), &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let text = stdout(&output);
    assert!(text.contains("soft (promoted):"), "{text}");
    let line = detail(&text);
    assert!(
        line.contains(
            "soft edits 6/5; promoted to blocking; 4 committed over-soft edits before main + 2 on this branch; this branch first reaches the promotion limit"
        ),
        "detail: {line}"
    );
    assert!(
        text.lines().any(|line| line == LEAD),
        "no base lead in:\n{text}"
    );
    let flat = folded(&text);
    assert!(
        flat.contains(
            "Split now or record the debt with `fissile exception add <path> --severity soft --rule <rule> --kind <kind>`."
        ),
        "{text}"
    );
    for claim in ["--no-verify", "staged", "unstage"] {
        assert!(
            !text.contains(claim),
            "base epilogue claims {claim:?}:\n{text}"
        );
    }

    let json = fissile(
        work.path(),
        &["check", "--base", "main", "--format", "json"],
    );
    assert_eq!(json.status.code(), Some(1), "{json:?}");
    let finding = json_finding(&json, "docs/spec.md");
    assert_eq!(finding["severity"], "soft");
    assert_eq!(finding["soft_edit_count"], 6);
    assert_eq!(finding["soft_edit_limit"], 5);
    assert_eq!(finding["soft_edit_history_complete"], true);
    assert_eq!(finding["promotion"], "soft_edit_limit");
    assert_eq!(finding["soft_edit_source"], "committed");
    assert_eq!(finding["soft_edit_before_base"], 4);
}

#[test]
fn a_limit_reached_before_the_base_says_so() {
    let work = branch_after("six-plus-one", 6);
    edit_spec(work.path(), 50, "topic edit 1");

    let output = fissile(work.path(), &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let line = detail(&stdout(&output));
    assert!(
        line.contains(
            "soft edits 7/5; promoted to blocking; 6 committed over-soft edits before main + 1 on this branch; edits before main already reached the promotion limit"
        ),
        "detail: {line}"
    );
}

#[test]
fn a_branch_that_crosses_soft_starts_the_run_without_promotion() {
    let work = init("starts-run");
    edit_spec(work.path(), 10, "under soft on main");
    git(work.path(), &["checkout", "-q", "-b", "topic"]);
    edit_spec(work.path(), 30, "crossing edit");

    let output = fissile(work.path(), &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let line = detail(&stdout(&output));
    assert!(
        line.contains(
            "soft edits 1/5; 0 committed over-soft edits before main + 1 on this branch; this branch starts the over-soft run"
        ),
        "detail: {line}"
    );
    assert!(!line.contains("promoted"), "detail: {line}");

    let json = fissile(
        work.path(),
        &["check", "--base", "main", "--format", "json"],
    );
    assert_eq!(json.status.code(), Some(0), "{json:?}");
    let finding = json_finding(&json, "docs/spec.md");
    assert_eq!(finding["soft_edit_count"], 1);
    assert_eq!(finding["soft_edit_before_base"], 0);
    assert_eq!(finding["soft_edit_source"], "committed");
    assert!(finding.get("promotion").is_none(), "{finding}");
}

#[test]
fn a_branch_edit_at_soft_resets_the_inherited_run() {
    let work = branch_after("reset", 4);
    edit_spec(work.path(), 20, "shrink to the soft limit");
    edit_spec(work.path(), 31, "cross again");

    let output = fissile(work.path(), &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let line = detail(&stdout(&output));
    assert!(
        line.contains(
            "soft edits 1/5; 0 committed over-soft edits before main + 1 on this branch; this branch starts the over-soft run"
        ),
        "the base never resets the run, a version at soft does: {line}"
    );
}

/// A shallow clone whose only over-soft edit sits on its boundary: the count it
/// can establish is zero, the finding stays advisory, and nothing underflows.
#[test]
fn incomplete_history_with_a_zero_count_stays_advisory() {
    let source = init("shallow-source");
    edit_spec(source.path(), 10, "under soft at the base");
    git(source.path(), &["checkout", "-q", "-b", "side"]);
    edit_spec(source.path(), 30, "side crosses soft");
    git(source.path(), &["checkout", "-q", "main"]);
    git(
        source.path(),
        &[
            "-c",
            "user.email=e2e@fissile.invalid",
            "-c",
            "user.name=e2e",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=e2e-no-hooks",
            "merge",
            "-q",
            "--no-ff",
            "-m",
            "merge side",
            "side",
        ],
    );

    let work = Work::new("shallow-clone");
    let url = format!("file://{}", source.path().display());
    git(work.path(), &["clone", "-q", "--depth", "2", &url, "repo"]);
    let repo = work.path().join("repo");
    assert!(repo.join(".git/shallow").is_file(), "the clone is shallow");

    let output = fissile(&repo, &["check", "--base", "HEAD^1", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(stderr(&output).is_empty(), "{output:?}");
    let line = detail(&stdout(&output));
    assert!(
        line.contains(
            "soft edits 0/5; history incomplete; promotion disabled; at least 0 committed over-soft edits before HEAD^1 + 0 on this branch established"
        ),
        "detail: {line}"
    );
    for unproved in [
        "starts the over-soft run",
        "first reaches",
        "already reached",
    ] {
        assert!(!line.contains(unproved), "unproved boundary: {line}");
    }

    let json = fissile(&repo, &["check", "--base", "HEAD^1", "--format", "json"]);
    assert_eq!(json.status.code(), Some(0), "{json:?}");
    let finding = json_finding(&json, "docs/spec.md");
    assert_eq!(finding["soft_edit_count"], 0);
    assert_eq!(finding["soft_edit_before_base"], 0);
    assert_eq!(finding["soft_edit_history_complete"], false);
    assert_eq!(finding["soft_edit_source"], "committed");
    assert!(finding.get("promotion").is_none(), "{finding}");
}

/// The index and the working tree are not the judged content and are left as
/// they were: the run measures `HEAD`'s blobs (§FS-004-check-audit.1.5).
#[test]
fn the_index_and_working_tree_are_neither_judged_nor_touched() {
    let work = branch_after("isolation", 4);
    edit_spec(work.path(), 40, "topic edit 1");
    edit_spec(work.path(), 41, "topic edit 2");
    let root = work.path();
    // The working tree shrinks the file under soft, unstaged; the index carries a
    // different, over-soft file that is not in any commit.
    write_lines(root, "docs/spec.md", 5, "unstaged shrink");
    write_lines(root, "docs/staged.md", 90, "staged only");
    git(root, &["add", "docs/staged.md"]);

    let index = fs::read(root.join(".git/index")).unwrap();
    let worktree = fs::read(root.join("docs/spec.md")).unwrap();
    let head = fs::read_to_string(root.join(".git/HEAD")).unwrap();

    let output = fissile(root, &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(1), "HEAD is judged: {output:?}");
    let text = stdout(&output);
    assert!(detail(&text).contains("soft edits 6/5"), "{text}");
    assert!(
        !text.contains("docs/staged.md"),
        "the index is not selected:\n{text}"
    );

    assert_eq!(
        fs::read(root.join(".git/index")).unwrap(),
        index,
        "index changed"
    );
    assert_eq!(fs::read(root.join("docs/spec.md")).unwrap(), worktree);
    assert_eq!(fs::read_to_string(root.join(".git/HEAD")).unwrap(), head);
}
