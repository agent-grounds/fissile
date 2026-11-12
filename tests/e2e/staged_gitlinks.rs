//! Staged gitlinks have no file content, independently of object availability
//! (§FS-004-check-audit.1.6). Both commands share this selection (§FS-007-measure.1).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::base_support::{CONFIG, Work, fissile, stderr, stdout, write_lines};

const LINK: &str = "vendor/sub";
const NEIGHBOR: &str = "docs/café notes.md";
const EXCLUDED: &str = "docs/generated/ignored.md";

fn git_output(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_AUTHOR_NAME", "fissile-104")
        .env("GIT_AUTHOR_EMAIL", "e2e@fissile.invalid")
        .env("GIT_COMMITTER_NAME", "fissile-104")
        .env("GIT_COMMITTER_EMAIL", "e2e@fissile.invalid")
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs")
}

fn git(root: &Path, args: &[&str]) -> String {
    let output = git_output(root, args);
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        diagnostic(&output)
    );
    stdout(&output).trim().to_owned()
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", message]);
}

fn diagnostic(output: &Output) -> String {
    format!(
        "exit: {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        stdout(output),
        stderr(output)
    )
}

enum Pointer {
    Added,
    Bumped,
    Stored,
}

struct Fixture {
    _work: Work,
    host: PathBuf,
    has_gitmodules: bool,
}

fn fixture(name: &str, pointer: Pointer, neighbor_lines: Option<usize>) -> Fixture {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .expect("HOME or USERPROFILE names the scratch filesystem");
    let work = Work(PathBuf::from(home).join(format!(
        "ag/tmp/fissile-104-e2e-{}-{name}",
        std::process::id()
    )));
    fs::create_dir_all(&work.0).unwrap();
    let host = work.0.join("host");
    fs::create_dir_all(host.join(".agent-grounds")).unwrap();
    // If Git can print a commit here, measuring it would trigger a hard finding.
    let config = format!(
        "{CONFIG}\n[[rules]]\nid = \"pointer-trap\"\ninclude = [\"vendor/**\"]\nunit = \"bytes\"\nsoft = 1\nhard = 2\nmessage = \"split-spec\"\n"
    );
    fs::write(host.join(".agent-grounds/fissile.toml"), config).unwrap();
    git(&host, &["init", "-q", "-b", "main"]);
    if !matches!(pointer, Pointer::Stored) {
        write_lines(&host, "vendor/sub/CVE-1.json", 1, "tracked corpus");
    }
    commit(&host, "host baseline");

    let has_gitmodules = matches!(pointer, Pointer::Added);
    if !matches!(pointer, Pointer::Stored) {
        let source = work.0.join("source");
        fs::create_dir_all(&source).unwrap();
        git(&source, &["init", "-q", "-b", "main"]);
        write_lines(&source, "README.md", 1, "external corpus");
        commit(&source, "external baseline");
        git(&host, &["rm", "-qr", LINK]);
        git(
            &host,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                "-q",
                source.to_str().unwrap(),
                LINK,
            ],
        );
        if matches!(pointer, Pointer::Bumped) {
            commit(&host, "replace tracked directory with submodule");
            write_lines(&host.join(LINK), "README.md", 2, "bumped corpus");
            commit(&host.join(LINK), "bump corpus");
            git(&host, &["add", "--", LINK]);
        }
        let oid = git(&host.join(LINK), &["rev-parse", "HEAD"]);
        let absent = git_output(&host, &["cat-file", "-e", &format!("{oid}^{{commit}}")]);
        assert!(
            !absent.status.success(),
            "commit must be absent from host store"
        );
        let show = git_output(&host, &["show", &format!(":{LINK}")]);
        assert!(
            !show.status.success(),
            "external gitlink must not resolve in host"
        );
    } else {
        let oid = git(&host, &["rev-parse", "HEAD"]);
        git(
            &host,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{oid},{LINK}"),
            ],
        );
        assert_eq!(git(&host, &["cat-file", "-t", &oid]), "commit");
        let show = git_output(&host, &["show", &format!(":{LINK}")]);
        assert!(show.status.success(), "{}", diagnostic(&show));
        assert!(
            show.stdout.len() > 2,
            "commit text must exceed the trap's hard budget"
        );
        assert!(
            stdout(&show).contains("diff --git"),
            "fixture must print a patch"
        );
    }
    let entry = git(&host, &["ls-files", "--stage", "--", LINK]);
    assert!(entry.starts_with("160000 "), "{entry}");
    let status = git(&host, &["diff", "--cached", "--name-status", "--", LINK]);
    let expected = if matches!(pointer, Pointer::Bumped) {
        'M'
    } else {
        'A'
    };
    assert!(
        status.starts_with(expected),
        "expected {expected}: {status}"
    );

    if let Some(lines) = neighbor_lines {
        write_lines(&host, NEIGHBOR, lines, "indexed neighbor");
        write_lines(&host, EXCLUDED, 202, "excluded neighbor");
        git(&host, &["add", "--", NEIGHBOR, EXCLUDED]);
        // The working copy gives the opposite verdict to the indexed content.
        write_lines(
            &host,
            NEIGHBOR,
            if lines > 200 { 1 } else { 201 },
            "unstaged",
        );
    }
    Fixture {
        _work: work,
        host,
        has_gitmodules,
    }
}

fn verify(name: &str, pointer: Pointer, neighbor_lines: Option<usize>, measure: bool) {
    let fixture = fixture(name, pointer, neighbor_lines);
    let args = if measure {
        &["measure", "--staged", "--format", "json"][..]
    } else {
        &["check", "--staged", "--no-color"][..]
    };
    let output = fissile(&fixture.host, args);
    let detail = diagnostic(&output);
    let over_hard = neighbor_lines.is_some_and(|lines| lines > 200);
    assert_eq!(
        output.status.code(),
        Some(i32::from(!measure && over_hard)),
        "{detail}"
    );
    assert!(output.stderr.is_empty(), "{detail}");
    if measure {
        let records: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let rows = records.as_array().expect("measurement array");
        assert!(
            rows.iter().all(|row| row["path"] != LINK),
            "gitlink was measured: {detail}"
        );
        let expected_rows =
            usize::from(fixture.has_gitmodules) + usize::from(neighbor_lines.is_some());
        assert_eq!(rows.len(), expected_rows, "{detail}");
        if let Some(lines) = neighbor_lines {
            let row = rows
                .iter()
                .find(|row| row["path"] == NEIGHBOR)
                .expect("regular indexed neighbor");
            assert_eq!(row["actual"], lines, "{detail}");
            assert_eq!(row["unit"], "lines", "{detail}");
            assert_eq!(row["rule_id"], "citable-spec", "{detail}");
        }
        assert!(rows.iter().all(|row| row["path"] != EXCLUDED), "{detail}");
    } else if over_hard {
        let text = stdout(&output);
        assert!(text.contains("hard: 1 file"), "{detail}");
        assert!(
            text.contains(&format!("{NEIGHBOR}: 201 non-blank lines (budget 200;")),
            "{detail}"
        );
        assert!(!text.contains(LINK), "{detail}");
        assert!(!text.contains(EXCLUDED), "{detail}");
    } else {
        assert_eq!(stdout(&output), "ok\n", "{detail}");
    }
}

#[test]
fn added_submodule_check() {
    verify("add-check", Pointer::Added, Some(3), false);
}

#[test]
fn added_submodule_measure() {
    verify("add-measure", Pointer::Added, Some(3), true);
}

#[test]
fn bumped_submodule_check() {
    verify("bump-check", Pointer::Bumped, Some(3), false);
}

#[test]
fn bumped_submodule_measure() {
    verify("bump-measure", Pointer::Bumped, Some(3), true);
}

#[test]
fn stored_gitlink_check() {
    verify("stored-check", Pointer::Stored, Some(3), false);
}

#[test]
fn stored_gitlink_measure() {
    verify("stored-measure", Pointer::Stored, Some(3), true);
}

#[test]
fn gitlink_only_check() {
    verify("only-check", Pointer::Stored, None, false);
}

#[test]
fn gitlink_only_measure() {
    verify("only-measure", Pointer::Stored, None, true);
}

#[test]
fn hard_neighbor_check() {
    verify("hard-check", Pointer::Stored, Some(201), false);
}

#[test]
fn hard_neighbor_measure() {
    verify("hard-measure", Pointer::Stored, Some(201), true);
}
