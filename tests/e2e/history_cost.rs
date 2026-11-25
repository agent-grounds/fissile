//! Long over-soft runs must preserve their full proof without pairwise Git
//! subprocesses (§FS-004-check-audit.1.4.1). Port of issue #114's linear history:
//! 400 non-blank Python lines, default soft/hard limits 350/550, N committed
//! version edits and one staged version edit. The shell interceptor is Unix-only.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

struct Work(PathBuf);

impl Work {
    fn new(mode: &str, versions: usize) -> Self {
        let scratch = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || PathBuf::from(std::env::var_os("HOME").expect("HOME")).join("ag/tmp"),
            PathBuf::from,
        );
        let root = scratch.join(format!(
            "fissile-114-history-cost-{}-{mode}-{versions}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn isolated(command: &mut Command) -> &mut Command {
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "history-cost")
        .env("GIT_AUTHOR_EMAIL", "history-cost@fissile.invalid")
        .env("GIT_COMMITTER_NAME", "history-cost")
        .env("GIT_COMMITTER_EMAIL", "history-cost@fissile.invalid")
}

fn git(root: &Path, args: &[&str]) {
    let output = isolated(Command::new("git").arg("-C").arg(root))
        .args(args)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn write_version(root: &Path, version: &str) {
    let mut body = format!("VERSION = \"{version}\"\n");
    for line in 2..=400 {
        body.push_str(&format!("field_{line} = {line}\n"));
    }
    fs::write(root.join("big.py"), body).unwrap();
}

fn fixture(mode: &str, versions: usize) -> Work {
    let work = Work::new(mode, versions);
    let repo = work.0.join("repo");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    for version in 1..=versions {
        write_version(&repo, &version.to_string());
        git(&repo, &["add", "big.py"]);
        git(&repo, &["commit", "-qm", &format!("bump {version}")]);
        if version == versions / 2 {
            git(&repo, &["tag", "base"]);
        }
    }
    write_version(&repo, "staged");
    git(&repo, &["add", "big.py"]);
    work
}

struct Observation {
    output: Output,
    processes: usize,
    merge_bases: usize,
    millis: u128,
}

fn measured(work: &Work, mode: &str, format: &str) -> Observation {
    let bin = work.0.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let wrapper = bin.join("git");
    // Paths travel as environment values, never interpolated shell source.
    fs::write(
        &wrapper,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$FISSILE_TEST_GIT_LOG\"\nexec \"$FISSILE_TEST_REAL_GIT\" \"$@\"\n",
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("resolve real git");
    assert!(real.status.success(), "{real:?}");
    let real = String::from_utf8(real.stdout).unwrap();
    let log = work.0.join("git.log");
    fs::write(&log, "").unwrap();
    let mut paths = vec![bin];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let mut command = Command::new(env!("CARGO_BIN_EXE_fissile"));
    isolated(&mut command)
        .current_dir(work.0.join("repo"))
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("FISSILE_TEST_REAL_GIT", real.trim())
        .env("FISSILE_TEST_GIT_LOG", &log)
        .arg("check");
    if mode == "staged" {
        command.arg("--staged");
    } else {
        command.args(["--base", "base"]);
    }
    let start = Instant::now();
    let output = command
        .args(["--format", format, "--no-color"])
        .output()
        .expect("fissile runs");
    let millis = start.elapsed().as_millis();
    let calls = fs::read_to_string(log).unwrap();
    let processes = calls.lines().count();
    assert!(processes > 0, "Git interception must be active");
    let merge_bases = calls
        .lines()
        .filter(|line| line.split_whitespace().any(|arg| arg == "merge-base"))
        .count();
    Observation {
        output,
        processes,
        merge_bases,
        millis,
    }
}

fn assert_semantics(output: &Output, mode: &str, format: &str, versions: usize) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let count = versions + usize::from(mode == "staged");
    if format == "json" {
        let records: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let records = records.as_array().expect("findings array");
        assert_eq!(records.len(), 1, "{records:?}");
        let finding = &records[0];
        assert_eq!(finding["path"], "big.py");
        assert_eq!(finding["actual"], 400);
        assert_eq!(finding["limit"], 350);
        assert_eq!(finding["severity"], "soft");
        assert_eq!(finding["soft_edit_count"], count);
        assert_eq!(finding["soft_edit_limit"], 5);
        assert_eq!(finding["soft_edit_history_complete"], true);
        assert_eq!(finding["promotion"], "soft_edit_limit");
        if mode == "committed" {
            assert_eq!(finding["soft_edit_source"], "committed");
            assert_eq!(finding["soft_edit_before_base"], versions / 2);
        }
    } else {
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            text.contains(&format!("soft edits {count}/5; promoted to blocking")),
            "{text}"
        );
        let provenance = if mode == "staged" {
            format!("{versions} prior committed over-soft edits + 1 staged edit")
        } else {
            assert!(!text.contains("staged edit"), "{text}");
            format!(
                "{} committed over-soft edits before base + {} on this branch",
                versions / 2,
                versions / 2
            )
        };
        assert!(text.contains(&provenance), "{text}");
        assert!(!text.contains("history incomplete"), "{text}");
    }
}

fn history_cost(mode: &str) {
    let mut excesses = Vec::new();
    for versions in [20, 40] {
        let work = fixture(mode, versions);
        for format in ["json", "text"] {
            let observation = measured(&work, mode, format);
            assert_semantics(&observation.output, mode, format, versions);
            // A generous linear budget allows bulk graph acquisition or bounded
            // per-record plumbing, without prescribing Git commands or storage.
            let bound = 4 * versions + 32;
            let detail = format!(
                "{mode} {format} N={versions}: full count/proof/promotion/exit correct; Git processes={} (merge-base={}), bound={bound}, {} ms",
                observation.processes, observation.merge_bases, observation.millis
            );
            eprintln!("{detail}");
            if observation.processes > bound {
                excesses.push(detail);
            }
        }
    }
    assert!(
        excesses.is_empty(),
        "history proof exceeded linear Git subprocess budget:\n{}",
        excesses.join("\n")
    );
}

#[test]
fn staged_history_has_linear_git_process_cost() {
    history_cost("staged");
}

#[test]
fn committed_history_has_linear_git_process_cost() {
    history_cost("committed");
}
