//! Shared-ID rules retain separate, accurate text blocks per measurement unit
//! (§FS-004-check-audit.1, §FS-001-config.3.2). Ports fissile.12's shipped config,
//! committed history and staged shrink; config rejection cannot satisfy a test.

use std::fs;
use std::path::PathBuf;

use super::base_support::{Work, commit, fissile, stdout};
use super::git;

/// Issue-named scratch, cleaned by the existing fixture's Drop implementation.
fn fixture(name: &str) -> Work {
    let scratch = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
        || {
            PathBuf::from(
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .expect("HOME or USERPROFILE"),
            )
            .join("ag/tmp")
        },
        PathBuf::from,
    );
    let work = Work(scratch.join(format!("fissile-99-{}-{name}", std::process::id())));
    fs::create_dir_all(work.path().join("src")).unwrap();
    git(work.path(), &["init", "-q", "-b", "main"]);
    let init = fissile(work.path(), &["init", ".", "--no-hook"]);
    assert_eq!(init.status.code(), Some(0), "{init:?}");
    let config = work.path().join(".agent-grounds/fissile.toml");
    // `lines()` drops the CRLF a Windows build's `include_str!` template carries.
    let original = fs::read_to_string(&config).unwrap();
    let lines: Vec<_> = original.lines().collect();
    assert!(lines.contains(&"soft = 350") && lines.contains(&"hard = 550"));
    let mut text = lines
        .iter()
        .map(|line| match *line {
            "soft = 350" => "soft = 500",
            "hard = 550" => "hard = 2500",
            other => other,
        })
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text.push_str(
        r#"
[[rules]]
id = "source"
include = ["**/*.rs"]
unit = "bytes"
soft = 1000
message = "split-source-soft"
"#,
    );
    fs::write(config, text).unwrap();
    for size in [450, 600, 605, 608, 611] {
        write_suite(&work, size);
        commit(work.path(), &format!("suite at {size}"));
    }
    write_suite(&work, 565);
    git(work.path(), &["add", "-A"]);
    work
}

fn write_suite(work: &Work, size: usize) {
    let mut text = String::new();
    for line in 1..=size {
        text.push_str(&format!("fn t_{line}() {{}}\n"));
        if line % 10 == 0 {
            text.push('\n');
        }
    }
    fs::write(work.path().join("src/suite.rs"), text).unwrap();
}

fn assert_separate_blocks(text: &str, promoted: bool) {
    let severity = if promoted { "soft (promoted)" } else { "soft" };
    let blocks: Vec<_> = text
        .split("\n\n")
        .filter(|block| block.starts_with(&format!("{severity}:")))
        .collect();
    assert_eq!(
        blocks.len(),
        2,
        "expected separate byte and line blocks:\n{text}"
    );
    for (block, budget, detail) in [
        (blocks[0], "1000-byte", "    src/suite.rs: 7858 bytes ("),
        (
            blocks[1],
            "500-line",
            "    src/suite.rs: 565 non-blank lines (budget 500;",
        ),
    ] {
        assert_eq!(
            block.lines().next().unwrap(),
            format!(
                "{severity}: 1 file over the {budget} budget [rule: source, message: split-source-soft]"
            )
        );
        let details: Vec<_> = block
            .lines()
            .filter(|line| line.starts_with("    "))
            .collect();
        assert_eq!(details.len(), 1, "one detail per one-file block:\n{block}");
        assert!(
            details[0].starts_with(detail),
            "wrong unit detail:\n{block}"
        );
        assert_eq!(
            details[0].contains("promoted to blocking"),
            promoted,
            "{block}"
        );
    }
}

#[test]
fn issue_99_snapshot_check_separates_shared_id_units() {
    let work = fixture("snapshot");
    let output = fissile(work.path(), &["check", "src/suite.rs", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_separate_blocks(&stdout(&output), false);
}

#[test]
fn issue_99_promoted_staged_check_separates_shared_id_units() {
    let work = fixture("staged");
    let output = fissile(work.path(), &["check", "--staged", "--no-color"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_separate_blocks(&stdout(&output), true);
}

#[test]
fn issue_99_audit_separates_shared_id_units() {
    let work = fixture("audit");
    let output = fissile(work.path(), &["audit", "--no-color"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_separate_blocks(&stdout(&output), false);
}
