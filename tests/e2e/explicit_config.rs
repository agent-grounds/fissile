//! An explicit `--config` is not discovery: a relative path is resolved once,
//! against the directory the command was started in, and the run still holds
//! the repository root's paths to it. The one resolved file is read for the
//! current tree and, under `audit --history`, at every revision
//! (§FS-001-config.8.1, §FS-004-check-audit.2.1).

use std::path::{Path, PathBuf};

use super::root_support::{FOUR_LINES, PY_CONFIG, Ran, Scratch, assert_none, same_verdict, write};

/// A config that lives in `sub/` and names `sub/` paths, as one kept beside the
/// code it governs would.
const LOCAL_CONFIG: &str = r#"fissile_config_version = 1

[scan]
include = ["sub"]
exclude = []
respect_gitignore = false

[[messages]]
id = "split"
text = "Split it."

[[rules]]
id = "sub-py"
include = ["sub/**/*.py"]
unit = "lines"
soft = 2
hard = 10
message = "split"
"#;

/// A repository with no config of its own, `sub/local.toml`, and a staged
/// four-line `sub/x.py`.
fn local(scratch: &Scratch) -> PathBuf {
    let root = scratch.repo("local");
    write(&root, "sub/local.toml", LOCAL_CONFIG);
    write(&root, "sub/x.py", FOUR_LINES);
    scratch.git(&root, &["add", "-A"]);
    root
}

fn quiet(ran: &Ran) -> Option<String> {
    (!ran.stderr.is_empty())
        .then(|| format!("an explicit config said something:\n{}", ran.report()))
}

#[test]
fn issue_103_an_explicit_config_is_resolved_from_the_caller_and_read_at_the_root() {
    let scratch = Scratch::new("explicit");
    let root = local(&scratch);
    let sub = root.join("sub");
    let absolute = sub.join("local.toml").display().to_string();
    let at_root = scratch.fissile(
        &root,
        &[
            "measure",
            "sub/x.py",
            "--config",
            "sub/local.toml",
            "--no-color",
        ],
    );
    assert_eq!(
        at_root.stdout,
        "sub/x.py 4 lines [sub-py] soft 2 hard 10 — 6 to hard\n",
        "control: {}",
        at_root.report()
    );
    let mut problems = Vec::new();
    for config in ["local.toml", absolute.as_str()] {
        let below = scratch.fissile(&sub, &["measure", "x.py", "--config", config, "--no-color"]);
        problems.extend(same_verdict(&at_root, &below));
        problems.extend(quiet(&below));
    }
    let scope_at_root = scratch.fissile(
        &root,
        &["check", "--config", "sub/local.toml", "--no-color"],
    );
    assert!(
        scope_at_root.stdout.contains("sub/x.py: 4 non-blank lines"),
        "control: {}",
        scope_at_root.report()
    );
    let scope_below = scratch.fissile(&sub, &["check", "--config", "local.toml", "--no-color"]);
    problems.extend(same_verdict(&scope_at_root, &scope_below));
    assert_none(
        &problems,
        "an explicit config from sub/ was not applied at the root",
    );
}

/// Guard, passes today: an explicit config is read as given and never warns,
/// even when it names the deprecated home (§FS-001-config.8.1).
#[test]
fn issue_103_guard_an_explicit_deprecated_home_never_warns_from_a_subdirectory() {
    let scratch = Scratch::new("explicit-agents");
    let root = scratch.repo("agents");
    write(&root, ".agents/fissile.toml", PY_CONFIG);
    write(&root, "sub/x.py", FOUR_LINES);
    scratch.git(&root, &["add", "sub/x.py"]);
    let absolute = root.join(".agents/fissile.toml").display().to_string();
    let mut problems = Vec::new();
    for config in ["../.agents/fissile.toml", absolute.as_str()] {
        let ran = scratch.fissile(
            &root.join("sub"),
            &["check", "--staged", "--config", config, "--no-color"],
        );
        if ran.code != Some(1) || !ran.stdout.contains("sub/x.py: 4 non-blank lines") {
            problems.push(format!("the staged file was not failed:\n{}", ran.report()));
        }
        problems.extend(quiet(&ran));
    }
    assert_none(
        &problems,
        "an explicit .agents/fissile.toml from sub/ misbehaved",
    );
}

/// History over two commits in which `sub/x.py` stays over the soft limit of
/// `sub/local.toml`: a soft finding the history can only age if it reads that
/// file at both revisions.
fn history_repo(scratch: &Scratch) -> PathBuf {
    let root = scratch.repo("history");
    write(&root, "sub/local.toml", LOCAL_CONFIG);
    write(&root, "sub/x.py", "a = 1\nb = 2\nc = 3\n");
    scratch.commit(&root, "first");
    write(&root, "sub/x.py", FOUR_LINES);
    scratch.commit(&root, "second");
    root
}

fn history(scratch: &Scratch, dir: &Path, config: &str, format: &[&str]) -> Ran {
    let mut args = vec!["audit", "--history", "HEAD~1..HEAD", "--config", config];
    args.extend_from_slice(format);
    scratch.fissile(dir, &args)
}

#[test]
fn issue_103_audit_history_reads_the_callers_explicit_config_at_every_revision() {
    let scratch = Scratch::new("explicit-history");
    let root = history_repo(&scratch);
    let mut problems = Vec::new();
    for format in [
        &["--only", "history", "--no-color"][..],
        &["--format", "json"][..],
    ] {
        let at_root = history(&scratch, &root, "sub/local.toml", format);
        assert!(
            at_root.code == Some(0) && at_root.stdout.contains("sub-py"),
            "control: {}",
            at_root.report()
        );
        let below = history(&scratch, &root.join("sub"), "local.toml", format);
        problems.extend(same_verdict(&at_root, &below));
        problems.extend(quiet(&below));
    }
    assert_none(
        &problems,
        "audit --history from sub/ did not read sub/local.toml at every revision",
    );
}
