//! Where the root stops (§FS-001-config.8.1): a nested repository is its own
//! root even with no config, a linked worktree is its own checkout, and outside
//! Git the root is the directory the command was started in. A config passed
//! over below the root is named (§FS-001-config.8.4). `init` writes where it is
//! told, and says when that is below a work tree's root (§FS-002-init.1).

use std::path::Path;

use super::root_support::{FOUR_LINES, PY_CONFIG, Scratch, assert_none, write};

/// The `py` rule at a ceiling the reported file clears, so which config a run
/// read shows in its measurement.
fn loose_config() -> String {
    PY_CONFIG.replace("hard = 3", "hard = 100")
}

fn fallback(root: &Path) -> String {
    format!(
        "fissile: note: no .agent-grounds/fissile.toml in {}; using the built-in defaults\n",
        root.display()
    )
}

/// The §FS-001-config.8.4 line for `passed` in the invocation directory.
fn passed_over(passed: &str, in_effect: &str) -> String {
    format!(
        "fissile: warning: {passed} is ignored; discovery reads the repository root, \
         and {in_effect}\n"
    )
}

#[test]
fn issue_103_a_nested_repository_is_its_own_root_without_a_config() {
    let scratch = Scratch::new("nested");
    let outer = scratch.repo("outer");
    write(&outer, ".agent-grounds/fissile.toml", PY_CONFIG);
    let inner = outer.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    scratch.git(&inner, &["init", "-q", "-b", "main"]);
    write(&inner, "sub/x.py", FOUR_LINES);
    let ran = scratch.fissile(&inner.join("sub"), &["measure", "x.py", "--no-color"]);
    assert!(
        ran.code == Some(0)
            && ran.stdout.contains("sub/x.py 4 lines [source]")
            && !ran.stdout.contains("[py]")
            && ran.stderr == fallback(&inner),
        "the nested repository was not its own root on the built-in defaults:\n{}",
        ran.report()
    );
}

#[test]
fn issue_103_a_linked_worktree_reads_its_own_checkout() {
    let scratch = Scratch::new("worktree");
    let main = scratch.repo("main");
    write(&main, ".agent-grounds/fissile.toml", PY_CONFIG);
    write(&main, "sub/x.py", FOUR_LINES);
    scratch.commit(&main, "config");
    let linked = scratch.path().join("linked");
    let linked_arg = linked.display().to_string();
    scratch.git(
        &main,
        &["worktree", "add", "-q", "-b", "linked", &linked_arg],
    );
    // The main checkout's config now differs; the linked one keeps hard = 3.
    write(&main, ".agent-grounds/fissile.toml", &loose_config());
    let ran = scratch.fissile(&linked.join("sub"), &["measure", "x.py", "--no-color"]);
    assert!(
        ran.stdout == "sub/x.py 4 lines [py] soft 2 hard 3 — 1 over hard\n"
            && ran.stderr.is_empty(),
        "the linked worktree was not governed by its own config:\n{}",
        ran.report()
    );
}

#[test]
fn issue_103_outside_git_the_root_is_the_invocation_directory() {
    let scratch = Scratch::new("outside");
    let dir = scratch.path().join("plain");
    write(&dir, ".agent-grounds/fissile.toml", PY_CONFIG);
    write(&dir, "sub/x.py", FOUR_LINES);
    let mut problems = Vec::new();
    // Guard part: at the directory holding the config, it is read and nothing is said.
    let at_dir = scratch.fissile(&dir, &["measure", "sub/x.py", "--no-color"]);
    if !at_dir.stdout.contains("[py]") || !at_dir.stderr.is_empty() {
        problems.push(format!(
            "the config at the directory was not read:\n{}",
            at_dir.report()
        ));
    }
    // No ancestor is searched: the defaults apply, and the note names this directory.
    let below = scratch.fissile(&dir.join("sub"), &["measure", "x.py", "--no-color"]);
    if !below.stdout.starts_with("x.py ")
        || below.stdout.contains("[py]")
        || below.stderr != fallback(&dir.join("sub"))
    {
        problems.push(format!(
            "expected the defaults and their note:\n{}",
            below.report()
        ));
    }
    assert_none(
        &problems,
        "outside Git the root was not the invocation directory",
    );
}

#[test]
fn issue_103_a_config_below_the_root_is_named_as_passed_over() {
    let scratch = Scratch::new("passed-over");
    let up = "../.agent-grounds/fissile.toml is the config in effect";
    let mut problems = Vec::new();
    // (root homes, invocation-directory homes, expected stderr from sub/)
    let cases: [(&[&str], &[&str], String); 4] = [
        (
            &[".agent-grounds"],
            &[".agent-grounds"],
            passed_over(".agent-grounds/fissile.toml", up),
        ),
        (
            &[".agent-grounds"],
            &[".agents"],
            passed_over(".agents/fissile.toml", up),
        ),
        (
            &[".agent-grounds"],
            &[".agent-grounds", ".agents"],
            passed_over(".agent-grounds/fissile.toml", up),
        ),
        (
            &[".agents"],
            &[".agent-grounds"],
            "fissile: warning: ../.agents/fissile.toml is deprecated; \
             move it to ../.agent-grounds/fissile.toml\n"
                .to_owned()
                + &passed_over(
                    ".agent-grounds/fissile.toml",
                    "../.agents/fissile.toml is the config in effect",
                ),
        ),
    ];
    for (n, (root_homes, sub_homes, expected)) in cases.iter().enumerate() {
        let root = scratch.repo(&format!("case{n}"));
        for home in *root_homes {
            write(&root, &format!("{home}/fissile.toml"), PY_CONFIG);
        }
        for home in *sub_homes {
            write(&root, &format!("sub/{home}/fissile.toml"), &loose_config());
        }
        write(&root, "sub/x.py", FOUR_LINES);
        let below = scratch.fissile(&root.join("sub"), &["measure", "x.py", "--no-color"]);
        if !below.stdout.contains("soft 2 hard 3") || below.stderr != *expected {
            problems.push(format!("expected stderr {expected:?}:\n{}", below.report()));
        }
        // No such line at the root, or when --config names the document.
        let at_root = scratch.fissile(&root, &["measure", "sub/x.py", "--no-color"]);
        let named = format!("{}/fissile.toml", sub_homes[0]);
        let explicit = scratch.fissile(
            &root.join("sub"),
            &["measure", "x.py", "--config", &named, "--no-color"],
        );
        for ran in [at_root, explicit] {
            if ran.stderr.contains("discovery reads the repository root") {
                problems.push(format!(
                    "a passed-over warning where none is owed:\n{}",
                    ran.report()
                ));
            }
        }
    }
    assert_none(
        &problems,
        "a config below the root was not named as passed over",
    );
}

#[test]
fn issue_103_a_config_below_an_unconfigured_root_is_named_beside_the_fallback_note() {
    let scratch = Scratch::new("passed-over-defaults");
    let root = scratch.repo("defaults");
    write(&root, "sub/.agent-grounds/fissile.toml", PY_CONFIG);
    write(&root, "sub/x.py", FOUR_LINES);
    let below = scratch.fissile(&root.join("sub"), &["measure", "x.py", "--no-color"]);
    let expected = passed_over(
        ".agent-grounds/fissile.toml",
        "the built-in defaults are in effect",
    ) + &fallback(&root);
    assert!(
        below.stdout.contains("sub/x.py 4 lines [source]") && below.stderr == expected,
        "expected stderr {expected:?}:\n{}",
        below.report()
    );
}

#[test]
fn issue_103_init_below_a_work_tree_root_says_its_config_is_not_in_effect() {
    let scratch = Scratch::new("init");
    let root = scratch.repo("init");
    let sub = root.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    let warning = format!(
        "fissile: warning: {} is below the root of the git repository at {}; \
         fissile discovers config only at that root, so the config here is not in effect",
        sub.display(),
        root.display()
    );
    let mut problems = Vec::new();
    for args in [
        &["init", "--no-hook", "--dry-run"][..],
        &["init", "--no-hook"][..],
    ] {
        let below = scratch.fissile(&sub, args);
        if below.code != Some(0) || below.stderr.matches(&warning).count() != 1 {
            problems.push(format!("expected one `{warning}`:\n{}", below.report()));
        }
    }
    if !sub.join(".agent-grounds/fissile.toml").exists() {
        problems.push("init from sub/ did not write sub/.agent-grounds/fissile.toml".to_owned());
    }
    // Guard part: at the root there is nothing to say.
    let at_root = scratch.fissile(&root, &["init", "--no-hook"]);
    if at_root.code != Some(0) || at_root.stderr.contains("is below the root") {
        problems.push(format!("a warning at the root:\n{}", at_root.report()));
    }
    assert_none(&problems, "init below the root did not warn exactly once");
}
