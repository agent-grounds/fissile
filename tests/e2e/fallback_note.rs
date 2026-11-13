//! A run that holds the repository to the built-in defaults says so, in one note
//! on stderr naming the root it searched (§FS-001-config.8.5). The note is said
//! and never charged. The oracle is the same run with the built-in document
//! passed as an explicit `--config`. That run discovers nothing and owes no note,
//! so it has to match the discovered run in stdout and exit status, and its
//! stderr has to match everything after the note.

use std::path::{Path, PathBuf};

use super::root_support::{Ran, Scratch, assert_none, lines, write};

/// The built-in defaults, as the binary embeds them (§FS-001-config.0).
const BUILT_IN: &str = include_str!("../../src/templates/fissile.default.toml");

const REASON: &str = "The parser boundary does not exist yet, so this file cannot be split.";
const UNTIL: &str = "the parser module lands";

fn soft_entry(path: &str, max: usize) -> String {
    format!(
        "\n[[exceptions]]\npath = \"{path}\"\nmatch = \"exact\"\nrules = [\"source\"]\n\
         kind = \"deferred\"\nmax_accepted = {{ value = {max}, unit = \"lines\" }}\n\
         until = \"{UNTIL}\"\nreason = \"{REASON}\"\n"
    )
}

/// An unconfigured repository with a soft, an accepted, a hard and a stale case,
/// a `topic` branch one commit past `main`, and a staged edit on top.
fn unconfigured(scratch: &Scratch) -> PathBuf {
    let root = scratch.repo("bare");
    write(&root, "big.py", &lines(400, "big"));
    write(&root, "huge.py", &lines(600, "huge"));
    write(&root, "warm.py", &lines(400, "warm"));
    let registry = format!(
        "fissile_exceptions_version = 2\n{}{}",
        soft_entry("big.py", 400),
        soft_entry("gone.py", 400)
    );
    write(&root, "docs/file-size-agent-exceptions.toml", &registry);
    write(&root, "sub/x.py", "x = 1\n");
    scratch.commit(&root, "main");
    scratch.git(&root, &["checkout", "-q", "-b", "topic"]);
    write(&root, "warm.py", &lines(410, "warm"));
    scratch.commit(&root, "topic");
    write(&root, "warm.py", &lines(420, "warm"));
    scratch.git(&root, &["add", "warm.py"]);
    root
}

fn note(root: &Path) -> String {
    format!(
        "fissile: note: no .agent-grounds/fissile.toml in {}; using the built-in defaults\n",
        root.display()
    )
}

/// Every discovering command (§FS-001-config.8.2), in text and, where it has
/// one, in JSON.
fn discovering() -> Vec<Vec<&'static str>> {
    let mut runs: Vec<Vec<&str>> = Vec::new();
    let reporting: [&[&str]; 8] = [
        &["check", "warm.py", "huge.py"],
        &["check"],
        &["check", "--staged"],
        &["check", "--base", "main"],
        &["measure", "big.py", "warm.py"],
        &["measure", "--staged"],
        &["audit"],
        &["limits"],
    ];
    for args in reporting {
        runs.push([args, &["--no-color"]].concat());
        runs.push([args, &["--format", "json"]].concat());
    }
    runs.push(vec![
        "exception",
        "add",
        "warm.py",
        "--severity",
        "soft",
        "--rule",
        "source",
        "--kind",
        "deferred",
        "--reason",
        REASON,
        "--until",
        UNTIL,
        "--dry-run",
    ]);
    runs.push(vec![
        "exception",
        "retune",
        "big.py",
        "--severity",
        "soft",
        "--rule",
        "source",
        "--max",
        "500",
        "--unit",
        "lines",
        "--dry-run",
    ]);
    runs.push(vec![
        "exception",
        "remove",
        "gone.py",
        "--severity",
        "soft",
        "--rule",
        "source",
        "--dry-run",
    ]);
    runs
}

fn explicit(scratch: &Scratch, root: &Path, args: &[&str], config: &str) -> Ran {
    let mut with_config = args.to_vec();
    with_config.extend_from_slice(&["--config", config]);
    scratch.fissile(root, &with_config)
}

#[test]
fn issue_103_every_discovering_command_announces_the_built_in_defaults_once() {
    let scratch = Scratch::new("fallback");
    let root = unconfigured(&scratch);
    let defaults = scratch.path().join("defaults.toml");
    std::fs::write(&defaults, BUILT_IN).unwrap();
    let defaults = defaults.display().to_string();
    let note = note(&root);
    let mut problems = Vec::new();
    for args in discovering() {
        let oracle = explicit(&scratch, &root, &args, &defaults);
        let ran = scratch.fissile(&root, &args);
        if ran.stdout != oracle.stdout || ran.code != oracle.code {
            problems.push(format!(
                "the verdict moved; the oracle was\n{}\nbut got\n{}",
                oracle.report(),
                ran.report()
            ));
        } else if ran.stderr != format!("{note}{}", oracle.stderr) {
            problems.push(format!(
                "expected one note {note:?} first:\n{}",
                ran.report()
            ));
        }
    }
    assert_none(
        &problems,
        "a run on the built-in defaults did not announce them once",
    );
}

#[test]
fn issue_103_the_fallback_note_names_the_root_from_a_subdirectory() {
    let scratch = Scratch::new("fallback-below");
    let root = unconfigured(&scratch);
    let ran = scratch.fissile(&root.join("sub"), &["check", "--staged", "--no-color"]);
    assert!(
        ran.stdout.contains("warm.py") && ran.stderr == note(&root),
        "expected the note naming {}:\n{}",
        root.display(),
        ran.report()
    );
}

/// Guard, passes today: nothing was discovered, or the run aborted, so there is
/// no note (§FS-001-config.8.5).
#[test]
fn issue_103_guard_no_fallback_note_where_nothing_is_discovered() {
    let scratch = Scratch::new("fallback-silent");
    let root = unconfigured(&scratch);
    let mut problems = Vec::new();
    let quiet: [&[&str]; 3] = [
        &["--version"],
        &["check", "--help"],
        &["init", "--dry-run", "--no-hook"],
    ];
    for args in quiet {
        let ran = scratch.fissile(&root, args);
        if ran.code != Some(0) || ran.stderr.contains("fissile: note") {
            problems.push(ran.report());
        }
    }
    // A run-level error prints only its diagnostic (§FS-004-check-audit.5).
    write(
        &root,
        "docs/file-size-agent-exceptions.toml",
        "fissile_exceptions_version = = 2\n",
    );
    let aborted = scratch.fissile(&root, &["check", "--no-color"]);
    if aborted.code != Some(2) || aborted.stderr.contains("fissile: note") {
        problems.push(aborted.report());
    }
    assert_none(&problems, "a note where nothing was discovered");
}

/// Guard, passes today: a config that omits fields is the repository's own.
#[test]
fn issue_103_guard_a_config_that_omits_fields_prints_no_note() {
    let scratch = Scratch::new("fallback-sparse");
    let root = scratch.repo("sparse");
    write(
        &root,
        ".agent-grounds/fissile.toml",
        "fissile_config_version = 1\n",
    );
    write(&root, "big.py", &lines(600, "big"));
    let ran = scratch.fissile(&root, &["check", "--no-color"]);
    assert!(
        ran.code == Some(0) && ran.stderr.is_empty(),
        "a sparse config was announced:\n{}",
        ran.report()
    );
}
