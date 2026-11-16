//! Unit tests for `init`'s report rendering, entrypoint resolution, and managed
//! block handling (§FS-002-init). Kept in a sibling file so `init.rs` stays under
//! its own line budget, the way `config.rs` and `report.rs` already do.

use super::*;
use crate::config::Config;

#[test]
fn default_config_is_valid_and_fully_populated() {
    // The generated config must parse and build a checker (§DF-002-explicit-config).
    let config = Config::parse(DEFAULT_CONFIG).expect("default config parses");
    config
        .to_checker()
        .expect("default config builds a checker");
    // Every default is spelled out: defaults are present, not implied.
    assert!(DEFAULT_CONFIG.contains("respect_gitignore"));
    assert!(DEFAULT_CONFIG.contains("[output]"));
    assert!(DEFAULT_CONFIG.contains("count_blank_lines"));
    assert!(DEFAULT_CONFIG.contains("count_comment_lines"));
}

#[test]
fn starter_registries_parse() {
    assert!(DEFAULT_SOFT_REGISTRY.contains("fissile_exceptions_version = 2"));
    assert!(DEFAULT_HARD_REGISTRY.contains("fissile_exceptions_version = 2"));
}

fn report_for(entrypoints: &[&str]) -> Report {
    Report {
        outcomes: vec![Outcome {
            path: PathBuf::from(crate::config::CONFIG_HOME),
            action: Action::Wrote,
        }],
        config: PathBuf::from(crate::config::CONFIG_HOME),
        dry_run: false,
        hook: HookStatus::Installed,
        entrypoints: entrypoints.iter().map(PathBuf::from).collect(),
        deprecation: None,
    }
}

fn report_with_hook(hook: HookStatus) -> Report {
    Report {
        hook,
        ..report_for(&["AGENTS.md"])
    }
}

/// Step 2 reports the hook the run leaves in place, so it never promises
/// machinery that is not there (§FS-002-init.5). `--no-hook` is the path
/// that used to fall through to the promise (§FS-002-init.6).
#[test]
fn next_block_hook_step_matches_what_the_run_installed() {
    let installed = report_with_hook(HookStatus::Installed).render();
    assert!(installed.contains(NEXT_HOOK_STEP));

    let no_git = report_with_hook(HookStatus::SkippedNotGit).render();
    assert!(no_git.contains(NEXT_HOOK_STEP_NO_GIT));
    assert!(!no_git.contains(NEXT_HOOK_STEP));

    let no_hook = report_with_hook(HookStatus::SkippedByFlag).render();
    assert!(no_hook.contains(NEXT_HOOK_STEP_NO_HOOK));
    assert!(!no_hook.contains(NEXT_HOOK_STEP));
}

/// `--no-hook` declines the install; it does not remove the hook an earlier
/// run installed, so step 2 still reports the gate that is on disk
/// (§FS-002-init.5).
#[test]
fn no_hook_does_not_deny_a_hook_that_is_already_installed() {
    let root = std::env::temp_dir().join(format!("fissile-no-hook-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join(".git/hooks")).expect("a repo to install into");

    let mut options = InitOptions::new(&root);
    options.agents.agents_md = true;
    assert_eq!(
        run(&options).expect("first run").hook,
        HookStatus::Installed
    );

    options.hook = HookMode::Never;
    options.exceptions = true;
    let declined = run(&options).expect("second run");
    assert_eq!(declined.hook, HookStatus::Installed);
    assert!(declined.render().contains(NEXT_HOOK_STEP));
    let _ = fs::remove_dir_all(&root);
}

/// The closing line names an entrypoint the run handled, so it cannot send
/// the reader to a file that was never written (§FS-002-init.5).
#[test]
fn next_block_names_a_handled_entrypoint() {
    assert!(
        report_for(&["CLAUDE.md", ".claude/CLAUDE.md"])
            .render()
            .contains("see CLAUDE.md for what agents are told; the findings carry the rest.")
    );
    assert!(
        report_for(&["GEMINI.md"])
            .render()
            .contains("see GEMINI.md for what agents are told; the findings carry the rest.")
    );

    // Nothing to point at: the line is omitted, not invented.
    let rendered = report_for(&[]).render();
    assert!(rendered.contains("next:"));
    assert!(!rendered.contains("what agents are told"));
}

#[test]
fn appends_block_to_existing_file() {
    let (result, action) = apply_managed_block("# My Project\n\nHello.\n", Path::new("AGENTS.md"))
        .expect("append succeeds");
    assert_eq!(action, Action::Appended);
    let opened = "# My Project\n\nHello.\n\n<!-- BEGIN FISSILE MANAGED BLOCK -->";
    assert!(result.starts_with(opened));
    assert!(result.contains("fissile check --staged"));
}

#[test]
fn replaces_existing_managed_block_and_preserves_surroundings() {
    let existing = "# Project\n\n<!-- BEGIN FISSILE MANAGED BLOCK -->\n## Keeping Files Small With fissile (v3)\n\nold body\n<!-- END FISSILE MANAGED BLOCK -->\n\n## Other\n\nkeep me\n";
    let (result, action) =
        apply_managed_block(existing, Path::new("AGENTS.md")).expect("replace succeeds");
    assert_eq!(action, Action::Updated);
    assert!(result.contains("## Other\n\nkeep me"));
    assert!(!result.contains("old body"));
    assert!(result.starts_with("# Project\n"));
}

/// Markers carry no version, so a block whose heading this build cannot read
/// leaves nothing to judge it by. Assuming it is current would overwrite a
/// newer generation wholesale (§FS-002-init.4).
#[test]
fn rejects_a_delimited_block_that_declares_no_version() {
    let existing = "<!-- BEGIN FISSILE MANAGED BLOCK -->\n## Some Future Heading\n\nfuture body\n<!-- END FISSILE MANAGED BLOCK -->\n";
    let error = apply_managed_block(existing, Path::new("AGENTS.md"))
        .expect_err("an unreadable version is unsupported");
    assert!(matches!(
        error,
        InitError::UnsupportedBlock { version: None, .. }
    ));
}

#[test]
fn rejects_newer_block_version() {
    let existing = "<!-- BEGIN FISSILE MANAGED BLOCK -->\n## Keeping Files Small With fissile (v5)\n\nfuture body\n<!-- END FISSILE MANAGED BLOCK -->\n";
    let error = apply_managed_block(existing, Path::new("AGENTS.md")).expect_err("v5 unsupported");
    assert!(matches!(
        error,
        InitError::UnsupportedBlock {
            version: Some(5),
            ..
        }
    ));
}

/// The v3 block said any installed hook runs the same check as `--staged`, which
/// a path-passing hook does not. It is a supported earlier version, upgraded in
/// place to v4, which names the hook `init` installs instead (§FS-002-init.4).
#[test]
fn upgrades_a_v3_block_in_place() {
    let v3 = "<!-- BEGIN FISSILE MANAGED BLOCK -->\n## Keeping Files Small With fissile (v3)\n\nThis repository caps file size with [`fissile`](https://github.com/agent-grounds/fissile)\nso that agents spend fewer tokens reading. Run `fissile check --staged` before\nclaiming work is done; its findings say what to split and how. Where the\npre-commit hook is installed it runs that same check \u{2014} never get past it with\n`--no-verify`.\n<!-- END FISSILE MANAGED BLOCK -->\n";
    let existing = format!("# Project\n\n{v3}\n## Other\n\nkeep me\n");
    let (result, action) =
        apply_managed_block(&existing, Path::new("AGENTS.md")).expect("v3 is supported");
    assert_eq!(action, Action::Updated);
    assert!(result.contains("## Keeping Files Small With fissile (v4)"));
    assert!(!result.contains("(v3)"));
    assert!(!result.contains("runs that same check"));
    assert!(result.contains("keeps soft findings advisory"));
    assert!(result.starts_with("# Project\n"));
    assert!(result.contains("## Other\n\nkeep me"));
}

/// A v1/v2 block had no markers and ran to the next H1/H2. It is upgraded in
/// place, not left beside a second block (§FS-002-init.4).
#[test]
fn upgrades_a_legacy_heading_block_in_place() {
    let existing = "# Project\n\n## Keeping Files Small With fissile (v2)\n\nold body\n\n## Other\n\nkeep me\n";
    let (result, action) =
        apply_managed_block(existing, Path::new("AGENTS.md")).expect("upgrade succeeds");
    assert_eq!(action, Action::Updated);
    assert!(!result.contains("old body"));
    assert!(!result.contains("(v2)"));
    let heading = "## Keeping Files Small With fissile";
    assert_eq!(result.matches(heading).count(), 1);
    assert!(result.contains("## Other\n\nkeep me"));
}

/// A begin marker with no end falls back to the heading rule — and the
/// block's own heading must not end its own span (§FS-002-init.4).
#[test]
fn a_truncated_block_is_replaced_wholesale() {
    let existing = "<!-- BEGIN FISSILE MANAGED BLOCK -->\n## Keeping Files Small With fissile (v3)\n\nold body\n\n## Other\n\nkeep me\n";
    let (result, action) =
        apply_managed_block(existing, Path::new("AGENTS.md")).expect("replace succeeds");
    assert_eq!(action, Action::Updated);
    assert!(!result.contains("old body"));
    let heading = "## Keeping Files Small With fissile";
    assert_eq!(result.matches(heading).count(), 1);
    assert!(result.contains("## Other\n\nkeep me"));
}

/// The markers, not the next heading, bound the block: what a user writes
/// under it is theirs (§FS-002-init.4).
#[test]
fn a_user_heading_below_the_block_survives_a_refresh() {
    let block = MANAGED_BLOCK.trim_end();
    let existing = format!("{block}\n\n### Our own note\n\nkeep me\n");
    let (result, _) =
        apply_managed_block(&existing, Path::new("AGENTS.md")).expect("refresh succeeds");
    assert!(result.contains("### Our own note\n\nkeep me"));
}

#[test]
fn unchanged_block_reports_exists() {
    let existing = format!("# Project\n\n{}\n", MANAGED_BLOCK.trim_end());
    let (_, action) = apply_managed_block(&existing, Path::new("AGENTS.md")).expect("idempotent");
    assert_eq!(action, Action::Exists);
}
