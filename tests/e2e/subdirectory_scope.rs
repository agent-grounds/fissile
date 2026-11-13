//! What a config names, it names from the repository root, wherever the command
//! was started (§FS-001-config.8.1). `[scan].include` is walked from the root
//! (§FS-001-config.2), rule and exclusion globs match repository paths
//! (§FS-001-config.3.4, §FS-004-check-audit.1), and the token counter runs at
//! the root (§FS-001-config.7).

use std::path::{Path, PathBuf};

use super::root_support::{FOUR_LINES, Ran, Scratch, assert_none, lines, same_verdict, write};

/// `include = ["src"]`, a stale entry under `stale = "error"`, and a rule that
/// covers nothing, so the scope, the stale inventory and the coverage table all
/// say something.
const SCOPED_CONFIG: &str = r#"fissile_config_version = 1

[scan]
include = ["src"]
exclude = []
respect_gitignore = false

[exceptions]
soft_registry = "reg/soft.toml"
hard_registry = "reg/hard.toml"
stale = "error"

[[messages]]
id = "split"
text = "Split it."

[[rules]]
id = "rust"
include = ["src/**/*.rs"]
unit = "lines"
soft = 2
hard = 4
message = "split"

[[rules]]
id = "generated"
include = ["gen/**/*.rs"]
unit = "lines"
soft = 2
hard = 4
message = "split"
"#;

/// One live entry for a file outside `src/inner/` and one for a file that is
/// gone: a walk that stopped at the invocation directory would call both stale.
const STALE_ENTRY: &str = r#"fissile_exceptions_version = 2

[[exceptions]]
path = "src/c.rs"
match = "exact"
rules = ["rust"]
kind = "deferred"
max_accepted = { value = 3, unit = "lines" }
until = "the parser module lands"
reason = "The parser boundary does not exist yet, so this file cannot be split."

[[exceptions]]
path = "src/gone.rs"
match = "exact"
rules = ["rust"]
kind = "deferred"
max_accepted = { value = 3, unit = "lines" }
until = "the parser module lands"
reason = "The parser boundary does not exist yet, so this file cannot be split."
"#;

fn scoped(scratch: &Scratch) -> PathBuf {
    let root = scratch.repo("scoped");
    write(&root, ".agent-grounds/fissile.toml", SCOPED_CONFIG);
    write(&root, "reg/soft.toml", STALE_ENTRY);
    write(&root, "src/a.rs", &lines(6, "a"));
    write(&root, "src/c.rs", &lines(3, "c"));
    write(&root, "src/inner/b.rs", &lines(1, "b"));
    root
}

/// Run `args` at the root and from every listed subdirectory; each must give the
/// root's verdict and say nothing on stderr that the root did not say.
fn from_everywhere(scratch: &Scratch, root: &Path, dirs: &[&str], args: &[&str]) -> Vec<String> {
    let at_root = scratch.fissile(root, args);
    let mut problems = Vec::new();
    for dir in dirs {
        let below = scratch.fissile(&root.join(dir), args);
        problems.extend(same_verdict(&at_root, &below));
        if below.stderr != at_root.stderr {
            problems.push(format!(
                "stderr differs from the root's:\n{}",
                below.report()
            ));
        }
    }
    problems
}

fn control(ran: &Ran, code: i32, needle: &str) {
    assert!(
        ran.code == Some(code) && ran.stdout.contains(needle),
        "control: {}",
        ran.report()
    );
}

#[test]
fn issue_103_check_without_paths_walks_the_whole_scope_from_a_subdirectory() {
    let scratch = Scratch::new("scope-check");
    let root = scoped(&scratch);
    control(
        &scratch.fissile(&root, &["check", "--no-color"]),
        1,
        "src/a.rs: 6",
    );
    let mut problems = Vec::new();
    for args in [
        &["check", "--no-color"][..],
        &["check", "--format", "json"][..],
    ] {
        problems.extend(from_everywhere(
            &scratch,
            &root,
            &["src", "src/inner"],
            args,
        ));
    }
    assert_none(&problems, "check from src/ did not walk the root's scope");
}

#[test]
fn issue_103_audit_inventories_the_whole_scope_from_a_subdirectory() {
    let scratch = Scratch::new("scope-audit");
    let root = scoped(&scratch);
    let stale = scratch.fissile(&root, &["audit", "--stale-exceptions", "--no-color"]);
    control(&stale, 1, "src/gone.rs");
    let coverage = scratch.fissile(&root, &["audit", "--rule-coverage", "--no-color"]);
    control(&coverage, 1, "generated");
    let mut problems = Vec::new();
    let runs: [&[&str]; 3] = [
        &["audit", "--stale-exceptions", "--no-color"],
        &["audit", "--rule-coverage", "--no-color"],
        &[
            "audit",
            "--stale-exceptions",
            "--rule-coverage",
            "--format",
            "json",
        ],
    ];
    for args in runs {
        problems.extend(from_everywhere(
            &scratch,
            &root,
            &["src", "src/inner"],
            args,
        ));
    }
    assert_none(
        &problems,
        "audit from src/ did not inventory the root's scope",
    );
}

/// Rule globs and a named `[scan].exclude` both match the repository path, so
/// `sub/**` patterns govern `x.py` named from `sub/` (§FS-001-config.3.4).
const SUB_CONFIG: &str = r#"fissile_config_version = 1

[scan]
exclude = ["sub/gen/**"]
respect_gitignore = false

[[messages]]
id = "split"
text = "Split it."

[[rules]]
id = "sub-py"
include = ["sub/**/*.py"]
unit = "lines"
soft = 2
hard = 3
message = "split"
"#;

#[test]
fn issue_103_rule_and_exclusion_globs_match_repository_paths_from_a_subdirectory() {
    let scratch = Scratch::new("matching");
    let root = scratch.repo("matching");
    write(&root, ".agent-grounds/fissile.toml", SUB_CONFIG);
    write(&root, "sub/x.py", FOUR_LINES);
    write(&root, "sub/gen/big.py", FOUR_LINES);
    let sub = root.join("sub");
    let pairs: [(&[&str], &[&str], &str); 3] = [
        (
            &["measure", "sub/x.py", "--no-color"],
            &["measure", "x.py", "--no-color"],
            "[sub-py]",
        ),
        (
            &["check", "sub/x.py", "--no-color"],
            &["check", "x.py", "--no-color"],
            "sub/x.py: 4",
        ),
        // #106's context names the configured entry the file matches.
        (
            &["check", "sub/gen/big.py", "--no-color"],
            &["check", "gen/big.py", "--no-color"],
            "matches [scan].exclude \"sub/gen/**\"",
        ),
    ];
    let mut problems = Vec::new();
    for (at_root_args, below_args, needle) in pairs {
        let at_root = scratch.fissile(&root, at_root_args);
        assert!(
            at_root.stdout.contains(needle),
            "control: {}",
            at_root.report()
        );
        problems.extend(same_verdict(&at_root, &scratch.fissile(&sub, below_args)));
    }
    assert_none(
        &problems,
        "a sub/** glob did not match a path named from sub/",
    );
}

/// The token counter runs at the root, so a counter the config names by a
/// repository path is found from every directory (§FS-001-config.7).
#[cfg(unix)]
#[test]
fn issue_103_the_token_counter_runs_at_the_root_from_a_subdirectory() {
    let scratch = Scratch::new("tokens");
    let root = scratch.repo("tokens");
    let config = SUB_CONFIG
        .replace("unit = \"lines\"", "unit = \"tokens\"")
        .replace(
            "[[messages]]",
            "[tokens]\nenabled = true\ncommand = [\"sh\", \"tools/count.sh\", \"{path}\"]\n\n[[messages]]",
        );
    write(&root, ".agent-grounds/fissile.toml", &config);
    write(&root, "tools/count.sh", "wc -w < \"$1\"\n");
    write(&root, "sub/x.py", FOUR_LINES);
    let at_root = scratch.fissile(&root, &["measure", "sub/x.py", "--no-color"]);
    assert!(
        at_root.code == Some(0) && at_root.stdout.contains("sub/x.py 12 tokens [sub-py]"),
        "control: {}",
        at_root.report()
    );
    let below = scratch.fissile(&root.join("sub"), &["measure", "x.py", "--no-color"]);
    assert_none(
        &same_verdict(&at_root, &below)
            .into_iter()
            .collect::<Vec<_>>(),
        "the token counter did not run at the root",
    );
}
