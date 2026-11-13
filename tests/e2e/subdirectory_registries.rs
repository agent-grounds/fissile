//! Registries live where the config names them, relative to the repository root
//! (§FS-001-config.5), so every command reads them there and the exception
//! commands write them there from any directory. A path the caller types is
//! relative to where they stand and recorded relative to the repository. A
//! refused path is echoed as typed (§FS-003-exceptions.3, §FS-007-measure.1).

use std::fs;
use std::path::{Path, PathBuf};

use super::root_support::{FOUR_LINES, Scratch, assert_none, lines, same_verdict, write};

/// Custom registry paths, and a bump step of 1 so every ceiling is a measurement.
const REGISTRY_CONFIG: &str = r#"fissile_config_version = 1

[scan]
include = ["."]
exclude = []
respect_gitignore = false

[exceptions]
soft_registry = "reg/soft.toml"
hard_registry = "reg/hard.toml"

[exceptions.bump]
lines = 1

[[messages]]
id = "split"
text = "Split it."

[[rules]]
id = "py"
include = ["**/*.py"]
unit = "lines"
soft = 2
hard = 10
message = "split"
"#;

const REASON: &str = "The parser boundary does not exist yet, so this file cannot be split.";

/// One soft entry accepting `path` up to `max` lines.
fn entry(path: &str, matcher: &str, max: usize) -> String {
    format!(
        "\n[[exceptions]]\npath = \"{path}\"\nmatch = \"{matcher}\"\nrules = [\"py\"]\n\
         kind = \"deferred\"\nmax_accepted = {{ value = {max}, unit = \"lines\" }}\n\
         until = \"the parser module lands\"\nreason = \"{REASON}\"\n"
    )
}

/// A repository with the custom registries, `sub/x.py` at `x_lines`, and the
/// soft registry holding `entries`.
fn registered(scratch: &Scratch, name: &str, x_lines: usize, entries: &[String]) -> PathBuf {
    let root = scratch.repo(name);
    write(&root, ".agent-grounds/fissile.toml", REGISTRY_CONFIG);
    write(&root, "sub/x.py", &lines(x_lines, "x"));
    let mut registry = "fissile_exceptions_version = 2\n".to_owned();
    for entry in entries {
        registry.push_str(entry);
    }
    write(&root, "reg/soft.toml", &registry);
    root
}

fn soft_registry(root: &Path) -> String {
    fs::read_to_string(root.join("reg/soft.toml")).unwrap_or_default()
}

/// The soft ceiling the root registry records for exact `path`, if any.
fn ceiling(root: &Path, path: &str) -> Option<i64> {
    let registry: toml::Value = toml::from_str(&soft_registry(root)).ok()?;
    registry
        .get("exceptions")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("path").and_then(toml::Value::as_str) == Some(path))?
        .get("max_accepted")?
        .get("value")?
        .as_integer()
}

/// What a write from `sub/` must never leave behind: a registry beside the caller.
fn nothing_written_below(root: &Path) -> Option<String> {
    root.join("sub/reg")
        .exists()
        .then(|| "a registry was written under sub/reg".to_owned())
}

#[test]
fn issue_103_a_configured_registry_is_read_from_a_subdirectory() {
    let scratch = Scratch::new("registry-read");
    let root = registered(&scratch, "read", 4, &[entry("sub/x.py", "exact", 4)]);
    let at_root = scratch.fissile(&root, &["measure", "sub/x.py", "--no-color"]);
    assert_eq!(
        at_root.stdout,
        "sub/x.py 4 lines [py] soft 2 hard 10 soft-accepted 4 — 0 to soft-accepted\n",
        "control: {}",
        at_root.report()
    );
    let below = scratch.fissile(&root.join("sub"), &["measure", "x.py", "--no-color"]);
    assert_none(
        &same_verdict(&at_root, &below)
            .into_iter()
            .collect::<Vec<_>>(),
        "measure from sub/ did not read reg/soft.toml at the root",
    );
}

#[test]
fn issue_103_exception_commands_from_a_subdirectory_write_the_root_registry() {
    let scratch = Scratch::new("registry-write");
    let root = registered(&scratch, "write", 4, &[]);
    let sub = root.join("sub");
    let mut problems = Vec::new();

    let add = scratch.fissile(
        &sub,
        &[
            "exception",
            "add",
            "x.py",
            "--severity",
            "soft",
            "--rule",
            "py",
            "--kind",
            "deferred",
            "--reason",
            REASON,
            "--until",
            "the parser module lands",
        ],
    );
    if add.code != Some(0) || ceiling(&root, "sub/x.py") != Some(4) {
        problems.push(format!(
            "add did not record sub/x.py at 4:\n{}",
            add.report()
        ));
    }

    write(&root, "sub/x.py", &lines(6, "x"));
    let retune = scratch.fissile(
        &sub,
        &[
            "exception",
            "retune",
            "x.py",
            "--severity",
            "soft",
            "--rule",
            "py",
        ],
    );
    if retune.code != Some(0) || ceiling(&root, "sub/x.py") != Some(6) {
        problems.push(format!(
            "retune did not move sub/x.py to 6:\n{}",
            retune.report()
        ));
    }

    write(&root, "sub/x.py", &lines(1, "x"));
    let remove = scratch.fissile(
        &sub,
        &[
            "exception",
            "remove",
            "x.py",
            "--severity",
            "soft",
            "--rule",
            "py",
        ],
    );
    if remove.code != Some(0) || ceiling(&root, "sub/x.py").is_some() {
        problems.push(format!(
            "remove did not delete sub/x.py:\n{}",
            remove.report()
        ));
    }
    problems.extend(nothing_written_below(&root));
    assert_none(
        &problems,
        "an exception command from sub/ missed the root registry",
    );
}

/// `remove` measures the whole scope the entry's matcher covers, so an entry
/// still silencing a sibling outside `sub/` is kept (§FS-009-exception-remove.3).
#[test]
fn issue_103_remove_from_a_subdirectory_still_guards_a_sibling_outside_it() {
    let scratch = Scratch::new("registry-sibling");
    let root = registered(&scratch, "sibling", 1, &[entry("**/x.py", "glob", 4)]);
    write(&root, "other/x.py", FOUR_LINES);
    let before = soft_registry(&root);
    let args = [
        "exception",
        "remove",
        "**/x.py",
        "--match",
        "glob",
        "--severity",
        "soft",
        "--rule",
        "py",
    ];
    let at_root = scratch.fissile(&root, &args);
    assert!(
        at_root.code == Some(2) && at_root.stderr.contains("other/x.py"),
        "control: {}",
        at_root.report()
    );
    let below = scratch.fissile(&root.join("sub"), &args);
    let mut problems = Vec::new();
    if below.code != Some(2) || below.stderr != at_root.stderr {
        problems.push(format!(
            "expected the root's refusal naming other/x.py:\n{}",
            below.report()
        ));
    }
    if soft_registry(&root) != before {
        problems.push("the registry changed".to_owned());
    }
    assert_none(
        &problems,
        "remove from sub/ dropped an entry a sibling still needs",
    );
}

/// `../y.py` from `sub/` is inside the repository: it is `y.py` (§FS-007-measure.1).
#[test]
fn issue_103_a_parent_path_from_a_subdirectory_is_measured() {
    let scratch = Scratch::new("parent-path");
    let root = registered(&scratch, "parent", 1, &[]);
    write(&root, "y.py", FOUR_LINES);
    let at_root = scratch.fissile(&root, &["measure", "y.py", "--no-color"]);
    assert_eq!(
        at_root.stdout,
        "y.py 4 lines [py] soft 2 hard 10 — 6 to hard\n"
    );
    let mut problems = Vec::new();
    for command in ["measure", "check"] {
        let at_root = scratch.fissile(&root, &[command, "y.py", "--no-color"]);
        let below = scratch.fissile(&root.join("sub"), &[command, "../y.py", "--no-color"]);
        problems.extend(same_verdict(&at_root, &below));
    }
    assert_none(&problems, "../y.py from sub/ was not taken as y.py");
}

/// Guard, passes today: a refused argument is named exactly as the caller typed
/// it, never as the path it was rebased to (§FS-007-measure.1).
#[test]
fn issue_103_guard_a_refused_path_is_named_as_the_caller_typed_it() {
    let scratch = Scratch::new("refused-path");
    let root = registered(&scratch, "refused", 1, &[]);
    write(scratch.path(), "x.py", FOUR_LINES);
    let mut problems = Vec::new();
    for command in ["measure", "check"] {
        let ran = scratch.fissile(&root.join("sub"), &[command, "../../x.py", "--no-color"]);
        let named = format!("fissile {command}: path `../../x.py` is outside the repository");
        if ran.code != Some(2) || !ran.stderr.contains(&named) || ran.stderr.contains("`../x.py`") {
            problems.push(format!("expected `{named}`:\n{}", ran.report()));
        }
    }
    assert_none(&problems, "a refused path was not echoed as typed");
}
