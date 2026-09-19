//! `--help` stays a one-screen surface and subcommands carry examples
//! (§GOAL-003-friendly-output.3).

use std::process::Command;

/// The one-screen bound from §GOAL-003-friendly-output.3.
const MAX_HELP_LINES: usize = 24;

fn help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_fissile"))
        .args(args)
        .output()
        .expect("fissile runs");
    assert!(output.status.success(), "help exits zero for {args:?}");
    String::from_utf8(output.stdout).expect("help is utf-8")
}

#[test]
fn top_level_help_fits_one_screen() {
    let lines = help(&["--help"]).lines().count();
    assert!(
        lines <= MAX_HELP_LINES,
        "fissile --help is {lines} lines, over the {MAX_HELP_LINES}-line budget"
    );
}

#[test]
fn subcommand_help_fits_one_screen_and_shows_examples() {
    for command in ["init", "check", "measure", "audit", "limits", "exception"] {
        let text = help(&[command, "--help"]);
        let lines = text.lines().count();
        assert!(
            lines <= MAX_HELP_LINES,
            "fissile {command} --help is {lines} lines, over the {MAX_HELP_LINES}-line budget"
        );
        assert!(
            text.contains("examples:"),
            "fissile {command} --help should include compact examples"
        );
    }
}

/// §FS-004-check-audit.1.4: help distinguishes the index/history-backed mode
/// from path and no-path working-tree snapshots without exceeding one screen.
#[test]
fn issue_74_check_help_distinguishes_staged_history_from_snapshot_checks() {
    let text = help(&["check", "--help"]);
    let lines = text.lines().count();
    assert!(lines <= MAX_HELP_LINES, "check help has {lines} lines");
    for clause in [
        "--staged checks index bytes and Git history",
        "paths and plain check use working-tree snapshots",
    ] {
        assert!(text.contains(clause), "check help should state `{clause}`");
    }
}

/// §FS-006-cli.2: each `exception` subcommand carries its own one-screen usage,
/// so the shared screen stays a dispatcher rather than growing two flag lists.
#[test]
fn exception_subcommand_help_fits_one_screen() {
    for subcommand in ["add", "retune", "remove"] {
        let text = help(&["exception", subcommand, "--help"]);
        let lines = text.lines().count();
        assert!(
            lines <= MAX_HELP_LINES,
            "fissile exception {subcommand} --help is {lines} lines, \
             over the {MAX_HELP_LINES}-line budget"
        );
        assert!(
            text.contains("examples:"),
            "fissile exception {subcommand} --help should include compact examples"
        );
    }
}

/// §FS-006-cli.1: `exception` dispatches on its first argument, and the screen it
/// prints on a miss lists every subcommand there is — a surface that named two of
/// three is how §FS-009-exception-remove came to be missing for a release.
#[test]
fn the_exception_screen_lists_every_subcommand() {
    let text = help(&["exception", "--help"]);
    for subcommand in ["add", "retune", "remove"] {
        assert!(
            text.contains(&format!("\n  {subcommand} ")),
            "the exception usage screen should list `{subcommand}`"
        );
        // Each one answers `--help` for itself, so the shared screen stays a
        // dispatcher (§FS-006-cli.2).
        assert!(
            help(&["exception", subcommand, "--help"]).contains(&format!("exception {subcommand}")),
            "fissile exception {subcommand} --help should name the subcommand"
        );
    }
}

/// §FS-006-cli.2: the usage screen says what fissile is for, how to work with
/// it, and what to run to get the rest — the findings, not a document
/// (§DF-007-instructions-at-the-error-site.1). It is the one surface guaranteed
/// to exist in a repository whose agent entrypoint never received the block.
#[test]
fn top_level_help_states_the_workflow_and_points_at_the_instructions() {
    let text = help(&["--help"]);
    for clause in [
        "soft warns",
        "hard fails the commit",
        "fissile check --staged",
        "Its findings carry the rest",
    ] {
        assert!(
            text.contains(clause),
            "usage screen should state `{clause}`"
        );
    }
}
