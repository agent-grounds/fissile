//! Issue #103: run from a subdirectory, `fissile` read no config and held the
//! repository to the built-in defaults, so `check --staged` passed a file the
//! repository's own config fails. Discovery reads the root of the Git work tree
//! from wherever the command starts (§FS-001-config.8.1). Each scenario here
//! runs at the root and again from `sub/` and compares the two.

use std::path::PathBuf;

use super::root_support::{FOUR_LINES, PY_CONFIG, Scratch, assert_none, same_verdict, write};

const DEPRECATED_AT_ROOT: &str = "fissile: warning: .agents/fissile.toml is deprecated; move it to .agent-grounds/fissile.toml\n";
const IGNORED_AT_ROOT: &str = "fissile: warning: .agents/fissile.toml is ignored; \
                               .agent-grounds/fissile.toml is the config in effect\n";
const MEASURED_AT_ROOT: &str = "sub/x.py 4 lines [py] soft 2 hard 3 — 1 over hard\n";

/// The reported tree: the `py` rule in `home`, a staged four-line `sub/x.py`.
fn reported(scratch: &Scratch, home: &str) -> PathBuf {
    let root = scratch.repo(home.trim_start_matches('.'));
    write(&root, &format!("{home}/fissile.toml"), PY_CONFIG);
    write(&root, "sub/x.py", FOUR_LINES);
    scratch.git(&root, &["add", "sub/x.py"]);
    root
}

/// Separators folded to `/`: these scenarios pin which directory a path is
/// spelled from, not the platform's separator.
fn slashed(text: &str) -> String {
    text.replace('\\', "/")
}

/// The deprecation line, spelled from `up` levels below the root (§FS-001-config.8.2).
fn deprecated_from(up: &str) -> String {
    format!(
        "fissile: warning: {up}.agents/fissile.toml is deprecated; \
         move it to {up}.agent-grounds/fissile.toml\n"
    )
}

#[test]
fn issue_103_measure_from_a_subdirectory_reads_the_repository_config() {
    let scratch = Scratch::new("reported-measure");
    let mut problems = Vec::new();
    for home in [".agent-grounds", ".agents"] {
        let root = reported(&scratch, home);
        let at_root = scratch.fissile(&root, &["measure", "sub/x.py", "--no-color"]);
        assert_eq!(at_root.stdout, MEASURED_AT_ROOT, "{}", at_root.report());
        let below = scratch.fissile(&root.join("sub"), &["measure", "x.py", "--no-color"]);
        problems.extend(same_verdict(&at_root, &below));
        let warning = if home == ".agents" {
            deprecated_from("../")
        } else {
            String::new()
        };
        if slashed(&below.stderr) != warning {
            problems.push(format!("stderr should be {warning:?}:\n{}", below.report()));
        }
    }
    assert_none(
        &problems,
        "measure from sub/ did not read the root's config",
    );
}

#[test]
fn issue_103_check_staged_from_a_subdirectory_fails_what_the_root_fails() {
    let scratch = Scratch::new("reported-staged");
    let mut problems = Vec::new();
    for home in [".agent-grounds", ".agents"] {
        let root = reported(&scratch, home);
        let at_root = scratch.fissile(&root, &["check", "--staged", "--no-color"]);
        assert_eq!(at_root.code, Some(1), "control: {}", at_root.report());
        assert!(
            at_root
                .stdout
                .contains("sub/x.py: 4 non-blank lines (budget 3"),
            "control: {}",
            at_root.report()
        );
        let below = scratch.fissile(&root.join("sub"), &["check", "--staged", "--no-color"]);
        problems.extend(same_verdict(&at_root, &below));
    }
    assert_none(
        &problems,
        "check --staged from sub/ passed what the root fails",
    );
}

#[test]
fn issue_103_an_invalid_config_is_refused_from_a_subdirectory() {
    let scratch = Scratch::new("reported-invalid");
    let refusal = "rule py declares a soft limit with no message";
    let mut problems = Vec::new();
    for home in [".agent-grounds", ".agents"] {
        let root = reported(&scratch, home);
        let config = PY_CONFIG.replace("message = \"split\"\n", "");
        write(&root, &format!("{home}/fissile.toml"), &config);
        let at_root = scratch.fissile(&root, &["check", "--staged", "--no-color"]);
        assert!(
            at_root.code == Some(2) && at_root.stderr.contains(refusal),
            "control: {}",
            at_root.report()
        );
        let below = scratch.fissile(&root.join("sub"), &["check", "--staged", "--no-color"]);
        if below.code != Some(2) || !below.stderr.contains(refusal) {
            problems.push(below.report());
        }
    }
    assert_none(
        &problems,
        &format!("an invalid config was not refused from sub/ with `{refusal}`"),
    );
}

/// Guard, passes today: at the root, the bytes of the reported runs and of
/// both home warnings stay as they are, and the new home wins (§FS-001-config.8.1).
#[test]
fn issue_103_guard_runs_at_the_root_keep_their_bytes() {
    let scratch = Scratch::new("root-bytes");
    let mut problems = Vec::new();
    let both = reported(&scratch, ".agent-grounds");
    // The deprecated home would accept the file; it must not be the one read.
    write(
        &both,
        ".agents/fissile.toml",
        &PY_CONFIG.replace("hard = 3", "hard = 100"),
    );
    let cases = [
        (reported(&scratch, ".agents"), DEPRECATED_AT_ROOT),
        (both, IGNORED_AT_ROOT),
    ];
    for (root, warning) in cases {
        let ran = scratch.fissile(&root, &["measure", "sub/x.py", "--no-color"]);
        if ran.code != Some(0) || ran.stdout != MEASURED_AT_ROOT || ran.stderr != warning {
            problems.push(format!("expected {warning:?}:\n{}", ran.report()));
        }
    }
    assert_none(&problems, "a run at the root changed its bytes");
}

/// From below the root, the home warnings name their paths from where the
/// caller stands (§FS-001-config.8.2, §FS-001-config.8.3), and the new home still wins.
#[test]
fn issue_103_home_warnings_below_the_root_are_spelled_from_the_caller() {
    let scratch = Scratch::new("spelled");
    let mut problems = Vec::new();
    let deprecated = reported(&scratch, ".agents");
    std::fs::create_dir_all(deprecated.join("sub/deep")).unwrap();
    let cases = [
        (deprecated.join("sub"), "x.py", deprecated_from("../")),
        (
            deprecated.join("sub/deep"),
            "../x.py",
            deprecated_from("../../"),
        ),
    ];
    for (dir, path, warning) in cases {
        let ran = scratch.fissile(&dir, &["measure", path, "--no-color"]);
        if ran.stdout != MEASURED_AT_ROOT || slashed(&ran.stderr) != warning {
            problems.push(format!("expected stderr {warning:?}:\n{}", ran.report()));
        }
    }

    let both = reported(&scratch, ".agent-grounds");
    write(
        &both,
        ".agents/fissile.toml",
        &PY_CONFIG.replace("hard = 3", "hard = 100"),
    );
    let ignored = "fissile: warning: ../.agents/fissile.toml is ignored; \
                   ../.agent-grounds/fissile.toml is the config in effect\n";
    let ran = scratch.fissile(&both.join("sub"), &["measure", "x.py", "--no-color"]);
    if ran.stdout != MEASURED_AT_ROOT || slashed(&ran.stderr) != ignored {
        problems.push(format!("expected stderr {ignored:?}:\n{}", ran.report()));
    }
    assert_none(&problems, "a home warning below the root was misspelled");
}

/// A home that does not parse is an error naming it, never a fall-through to
/// the deprecated one (§FS-001-config.8.1), and below the root it is named from
/// where the caller stands.
#[test]
fn issue_103_an_unparsable_home_is_named_from_below_the_root() {
    let scratch = Scratch::new("unparsable");
    let root = reported(&scratch, ".agents");
    write(
        &root,
        ".agent-grounds/fissile.toml",
        "fissile_config_version = = 1\n",
    );
    let at_root = scratch.fissile(&root, &["measure", "sub/x.py", "--no-color"]);
    assert!(
        at_root.code == Some(2)
            && slashed(&at_root.stderr)
                .starts_with("fissile measure: ./.agent-grounds/fissile.toml: config parse error"),
        "control: {}",
        at_root.report()
    );
    let below = scratch.fissile(&root.join("sub"), &["measure", "x.py", "--no-color"]);
    assert!(
        below.code == Some(2)
            && slashed(&below.stderr)
                .starts_with("fissile measure: ../.agent-grounds/fissile.toml: config parse error")
            && below.stdout.is_empty(),
        "the unparsable home was not named as ../.agent-grounds/fissile.toml:\n{}",
        below.report()
    );
}
