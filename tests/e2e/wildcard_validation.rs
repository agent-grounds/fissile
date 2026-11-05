//! Wildcard entries obey the load-time ceiling contract (§FS-003-exceptions.4).
//! Port of issue 98's reproducer: the shipped config, source soft 500 / hard
//! 2500, the same five committed sizes, and the named-rule control before `*`.

use std::fs;

use super::base_support::{Work, commit, fissile, stderr, stdout};
use super::git;

const REGISTRY: &str = "docs/file-size-agent-exceptions.toml";

fn fixture(name: &str) -> Work {
    let work = Work::new(name);
    let root = work.path();
    git(root, &["init", "-q", "-b", "main"]);
    let init = fissile(root, &["init", ".", "--no-hook"]);
    assert!(init.status.success(), "{init:?}");
    let config = root.join(".agent-grounds/fissile.toml");
    // Windows builds embed the shipped config template with CRLF line endings.
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
    fs::write(config, text).unwrap();
    fs::create_dir_all(root.join("src")).unwrap();
    for count in [450, 600, 605, 608, 611] {
        let body: String = (1..=count)
            .map(|n| format!("fn t_{n}() {{}}\n{}", if n % 10 == 0 { "\n" } else { "" }))
            .collect();
        fs::write(root.join("src/suite.rs"), body).unwrap();
        commit(root, &format!("suite at {count}"));
    }
    work
}

fn assert_refused(work: &Work, args: &[&str], rule: &str) {
    fs::create_dir_all(work.path().join("docs")).unwrap();
    fs::write(
        work.path().join(REGISTRY),
        format!(
            r#"fissile_exceptions_version = 2
[[exceptions]]
path = "src/suite.rs"
match = "exact"
rules = ["{rule}"]
kind = "deferred"
until = "seam-exists"
max_accepted = {{ value = 300, unit = "lines" }}
reason = "the suite splits once its fixture seam exists"
"#
        ),
    )
    .unwrap();
    let output = fissile(work.path(), args);
    assert_eq!(
        output.status.code(),
        Some(2),
        "rules = [{rule:?}], args = {args:?}\nstdout:\n{}\nstderr:\n{}",
        stdout(&output),
        stderr(&output)
    );
    assert!(
        stderr(&output).contains(&format!(
            "{REGISTRY}: src/suite.rs max_accepted.value 300 is below rule source limit 500"
        )),
        "{}",
        stderr(&output)
    );
}

#[test]
fn audit_rejects_the_wildcard_300_line_ceiling_like_the_named_control() {
    let work = fixture("wildcard-98-audit");
    for rule in ["source", "*"] {
        assert_refused(&work, &["audit", "--no-color"], rule);
    }
}

#[test]
fn check_rejects_the_wildcard_300_line_ceiling_like_the_named_control() {
    let work = fixture("wildcard-98-check");
    for rule in ["source", "*"] {
        assert_refused(&work, &["check", "src/suite.rs", "--no-color"], rule);
    }
}
