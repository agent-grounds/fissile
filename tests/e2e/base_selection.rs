//! What `check --base <ref>` selects, which endpoints it refuses, and how it
//! combines with the other input modes (§FS-004-check-audit.1.5), plus the stale
//! entries a base range proves (§FS-004-check-audit.1.3). Every case here fails
//! today: `--base` is an unknown option, exit 2.

use super::base_support::{
    Work, branch_after, commit, edit_spec, fissile, folded, init, stderr, stdout, write_lines,
};
use super::git;

#[test]
fn an_equal_base_selects_nothing() {
    // `HEAD` is `main`, whose file is promoted-size debt a `--staged` crossing
    // would block on: an empty range judges no file at all.
    let work = branch_after("equal", 6);

    let text = fissile(work.path(), &["check", "--base", "main", "--no-color"]);
    assert_eq!(text.status.code(), Some(0), "{text:?}");
    assert_eq!(stdout(&text), "ok\n");
    assert!(stderr(&text).is_empty(), "{text:?}");

    let json = fissile(
        work.path(),
        &["check", "--base", "HEAD", "--format", "json"],
    );
    assert_eq!(json.status.code(), Some(0), "{json:?}");
    assert_eq!(stdout(&json).trim_end(), "[]");
}

#[test]
fn a_divergent_base_exits_2_and_names_the_fork_point() {
    let work = init("divergent");
    edit_spec(work.path(), 5, "shared");
    git(work.path(), &["checkout", "-q", "-b", "other"]);
    edit_spec(work.path(), 6, "only on other");
    git(work.path(), &["checkout", "-q", "main"]);
    edit_spec(work.path(), 7, "only on main");

    let output = fissile(work.path(), &["check", "--base", "other"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let diagnostic = stderr(&output);
    assert!(
        diagnostic.starts_with("fissile check: --base other: "),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("is not an ancestor of HEAD"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains(r#"fissile check --base "$(git merge-base other HEAD)""#),
        "{diagnostic}"
    );
}

/// A shallow clone cannot prove an ancestry that holds, and its merge-base is
/// empty, so the run names the cut history and no fork point
/// (§FS-004-check-audit.1.5).
#[test]
fn a_shallow_clone_names_unproven_ancestry_not_divergence() {
    let source = branch_after("unproven-ancestry-source", 4);
    edit_spec(source.path(), 34, "topic edit 1");
    edit_spec(source.path(), 35, "topic edit 2");
    let clone = Work::new("unproven-ancestry-clone");
    let from = source.path().to_str().expect("UTF-8 scratch path");
    let into = clone.path().to_str().expect("UTF-8 scratch path");
    // `--no-local` keeps `--depth`, which a plain-path clone would ignore.
    let clone_args = [
        "clone",
        "-q",
        "--no-local",
        "--depth=1",
        "--branch=topic",
        from,
        into,
    ];
    git(source.path(), &clone_args);
    let main = "main:refs/remotes/origin/main";
    git(
        clone.path(),
        &["fetch", "-q", "--depth", "1", "origin", main],
    );

    let output = fissile(clone.path(), &["check", "--base", "origin/main"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let diagnostic = stderr(&output);
    assert!(
        diagnostic.starts_with("fissile check: --base origin/main: cannot prove "),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("in a shallow clone"), "{diagnostic}");
    assert!(
        diagnostic.contains("git fetch --unshallow, or fetch-depth: 0 in CI"),
        "{diagnostic}"
    );
    assert!(
        !diagnostic.contains("is not an ancestor") && !diagnostic.contains("merge-base"),
        "{diagnostic}"
    );
}

#[test]
fn endpoints_that_name_no_single_commit_exit_2() {
    let work = branch_after("bad-endpoints", 1);
    edit_spec(work.path(), 40, "topic edit");

    let missing = fissile(work.path(), &["check", "--base", "origin/nope"]);
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    let diagnostic = stderr(&missing);
    assert!(
        diagnostic.starts_with("fissile check: --base origin/nope: "),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("fetch"),
        "a missing ref is told to fetch: {diagnostic}"
    );

    let range = fissile(work.path(), &["check", "--base", "main..HEAD"]);
    assert_eq!(range.status.code(), Some(2), "{range:?}");
    assert!(
        stderr(&range).starts_with("fissile check: --base main..HEAD: "),
        "{range:?}"
    );

    // An expression is data, never an option.
    let option = fissile(work.path(), &["check", "--base", "--output=x"]);
    assert_eq!(option.status.code(), Some(2), "{option:?}");
    assert!(
        stderr(&option).starts_with("fissile check: --base --output=x: "),
        "{option:?}"
    );
}

/// Gitlinks are skipped, `[scan].exclude` applies, and `[scan].include` does not.
#[test]
fn selection_skips_gitlinks_and_exclusions_but_not_traversal_scope() {
    let work = init("selection");
    let root = work.path();
    git(root, &["checkout", "-q", "-b", "topic"]);
    let head = String::from_utf8(
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let pointer = format!("160000,{},vendor/sub", head.trim());
    git(root, &["update-index", "--add", "--cacheinfo", &pointer]);
    write_lines(root, "docs/generated/big.md", 90, "generated");
    write_lines(root, "notes/big.md", 30, "outside the scan include");
    commit(root, "a submodule bump, a generated doc and a note");

    let output = fissile(root, &["check", "--base", "main", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        stderr(&output).is_empty(),
        "a gitlink is not measured: {output:?}"
    );
    let text = stdout(&output);
    assert!(
        text.contains("notes/big.md:"),
        "include does not narrow a base run:\n{text}"
    );
    assert!(!text.contains("docs/generated"), "exclude applies:\n{text}");
    assert!(!text.contains("vendor/sub"), "{text}");
}

#[test]
fn a_deletion_on_the_branch_proves_a_stale_entry() {
    let work = init("stale");
    let root = work.path();
    write_lines(root, "docs/old.md", 30, "accepted debt");
    std::fs::write(
        root.join("docs/file-size-agent-exceptions.toml"),
        r#"fissile_exceptions_version = 2

[[exceptions]]
path = "docs/old.md"
match = "exact"
rules = ["citable-spec"]
kind = "deferred"
max_accepted = { value = 100, unit = "lines" }
until = "the old spec is retired"
reason = """
Missing boundary: the old spec has no successor yet.
"""
"#,
    )
    .unwrap();
    commit(root, "accepted debt");
    git(root, &["checkout", "-q", "-b", "topic"]);
    git(root, &["rm", "-q", "docs/old.md"]);
    commit(root, "retire the old spec");

    let output = fissile(root, &["check", "--base", "main", "--no-color"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stale = \"error\" fails: {output:?}"
    );
    let text = stdout(&output);
    assert!(
        text.contains("stale: 1 exception accepts a file that is not there"),
        "{text}"
    );
    assert!(
        text.contains("docs/old.md [soft, rule: citable-spec]"),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|line| line == "`fissile check --base main` rejected the selected HEAD files."),
        "{text}"
    );
    assert!(
        folded(&text).contains(
            "Remove the exception entry above, or point it at the path its file moved to."
        ),
        "{text}"
    );
    assert!(!text.contains("--no-verify"), "{text}");
}

#[test]
fn base_refuses_other_input_modes_and_malformed_values() {
    let work = branch_after("arguments", 1);
    let root = work.path();
    for (args, message) in [
        (
            &["check", "--base", "main", "--staged"][..],
            "--base conflicts with --staged",
        ),
        (
            &["check", "--staged", "--base", "main"][..],
            "--base conflicts with --staged",
        ),
        (
            &["check", "--base", "main", "docs/spec.md"][..],
            "--base conflicts with paths",
        ),
        (
            &["check", "docs/spec.md", "--base", "main"][..],
            "--base conflicts with paths",
        ),
        (&["check", "--base"][..], "--base requires a value"),
        (&["check", "--base", ""][..], "--base requires a value"),
        (
            &["check", "--base", "main", "--base", "main"][..],
            "--base given more than once",
        ),
    ] {
        let output = fissile(root, args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        let diagnostic = stderr(&output);
        assert!(
            diagnostic.starts_with(&format!("fissile check: {message}\n")),
            "{args:?}: {diagnostic}"
        );
        assert!(
            diagnostic.contains("usage: fissile check"),
            "{args:?}: {diagnostic}"
        );
    }

    let measure = fissile(root, &["measure", "--base", "main"]);
    assert_eq!(measure.status.code(), Some(2), "{measure:?}");
    assert!(
        stderr(&measure).contains("unknown option `--base`"),
        "{measure:?}"
    );
}
