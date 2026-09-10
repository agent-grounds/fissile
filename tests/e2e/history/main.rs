//! A fixed-date, multi-commit repository pins audit's historical debt report
//! (§FS-004-check-audit.2.1). The fixture describes committed mutations; this
//! harness materializes the Git DAG and drives the published `fissile` binary.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;

mod regressions;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Timeline {
    commits: Vec<FixtureCommit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureCommit {
    name: String,
    date: String,
    #[serde(default)]
    files: Vec<FixtureFile>,
    #[serde(default)]
    moves: Vec<FixtureMove>,
    #[serde(default)]
    removes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureFile {
    path: String,
    content: Option<String>,
    source: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixtureMove {
    from: String,
    to: String,
}

struct Repo {
    root: PathBuf,
    commits: BTreeMap<String, String>,
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/e2e/history")
        .join(name)
}

fn scratch(label: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let parent = std::env::var_os("FISSILE_HISTORY_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").expect("HOME locates ~/f/tmp")).join("f/tmp")
        });
    fs::create_dir_all(&parent).expect("create history fixture scratch directory");
    parent.join(format!(
        "fissile-history-e2e-{}-{n}-{label}",
        std::process::id()
    ))
}

fn git(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs")
}

fn git_ok(root: &Path, args: &[&str]) -> String {
    let output = git(root, args);
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is utf-8")
        .trim()
        .to_owned()
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture file has a parent")).unwrap();
    fs::write(path, content).unwrap();
}

fn build_timeline(name: &str) -> Repo {
    let fixture_path = fixture(name);
    let fixture_root = fixture_path.parent().unwrap();
    let raw = fs::read_to_string(&fixture_path).expect("read timeline fixture");
    let timeline: Timeline = toml::from_str(&raw).expect("parse timeline fixture");
    let root = scratch(name.trim_end_matches(".toml"));
    fs::create_dir_all(&root).unwrap();
    git_ok(&root, &["init", "-q", "-b", "main"]);

    let mut commits = BTreeMap::new();
    for commit in timeline.commits {
        for rename in commit.moves {
            let destination = root.join(&rename.to);
            fs::create_dir_all(destination.parent().unwrap()).unwrap();
            fs::rename(root.join(rename.from), destination).unwrap();
        }
        for relative in commit.removes {
            let path = root.join(relative);
            if path.is_dir() {
                fs::remove_dir_all(path).unwrap();
            } else {
                fs::remove_file(path).unwrap();
            }
        }
        for file in commit.files {
            let content = match (file.content, file.source) {
                (Some(content), None) => content,
                (None, Some(source)) => fs::read_to_string(fixture_root.join(source))
                    .expect("read sourced fixture file"),
                _ => panic!(
                    "fixture file {} needs exactly one of content or source",
                    file.path
                ),
            };
            write(&root, &file.path, &content);
        }

        git_ok(&root, &["add", "-A"]);
        let output = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args([
                "-c",
                "user.email=e2e@fissile.invalid",
                "-c",
                "user.name=e2e",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=e2e-no-hooks",
                "commit",
                "-q",
                "-m",
                &commit.name,
            ])
            .env("GIT_AUTHOR_DATE", &commit.date)
            .env("GIT_COMMITTER_DATE", &commit.date)
            .output()
            .expect("git commit runs");
        assert!(
            output.status.success(),
            "commit {} failed: {}",
            commit.name,
            String::from_utf8_lossy(&output.stderr)
        );
        let sha = git_ok(&root, &["rev-parse", "HEAD"]);
        git_ok(&root, &["tag", &commit.name, &sha]);
        commits.insert(commit.name, sha);
    }
    Repo { root, commits }
}

fn fissile(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("fissile runs")
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout is utf-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr is utf-8")
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("")
}

fn expand(template: &str, repo: &Repo) -> String {
    repo.commits
        .iter()
        .fold(template.to_owned(), |text, (name, sha)| {
            text.replace(&format!("${{{name}}}"), sha)
        })
}

fn expect_exact(
    failures: &mut Vec<String>,
    label: &str,
    output: &Output,
    expected_code: i32,
    expected_stdout: &str,
    expected_stderr: &str,
) {
    let actual_stdout = stdout(output);
    let actual_stderr = stderr(output);
    if code(output) != expected_code
        || actual_stdout != expected_stdout
        || actual_stderr != expected_stderr
    {
        failures.push(format!(
            "{label}: exit {} (want {expected_code}); stdout starts {:?}; stderr starts {:?}",
            code(output),
            first_line(&actual_stdout),
            first_line(&actual_stderr)
        ));
    }
}

fn expect_refusal(failures: &mut Vec<String>, label: &str, output: &Output, expected: &str) {
    let actual_stdout = stdout(output);
    let actual_stderr = stderr(output);
    if code(output) != 2 || !actual_stdout.is_empty() || !actual_stderr.contains(expected) {
        failures.push(format!(
            "{label}: exit {} (want 2), stdout starts {:?}, stderr starts {:?}; missing {:?}",
            code(output),
            first_line(&actual_stdout),
            first_line(&actual_stderr),
            expected
        ));
    }
}

fn add_non_ancestor(repo: &Repo) {
    git_ok(&repo.root, &["switch", "-q", "-c", "side", "base"]);
    write(&repo.root, "SIDE", "not an ancestor\n");
    git_ok(&repo.root, &["add", "SIDE"]);
    let output = Command::new("git")
        .arg("-C")
        .arg(&repo.root)
        .args([
            "-c",
            "user.email=e2e@fissile.invalid",
            "-c",
            "user.name=e2e",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=e2e-no-hooks",
            "commit",
            "-q",
            "-m",
            "side",
        ])
        .env("GIT_AUTHOR_DATE", "2024-01-04T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2024-01-04T00:00:00Z")
        .output()
        .unwrap();
    assert!(output.status.success());
    git_ok(&repo.root, &["tag", "side"]);
    git_ok(&repo.root, &["switch", "-q", "main"]);
}

fn shallow_clone(source: &Repo) -> Repo {
    let root = scratch("shallow");
    let url = format!("file://{}", source.root.display());
    let output = Command::new("git")
        .args(["clone", "-q", "--depth", "3", &url])
        .arg(&root)
        .output()
        .expect("git clone runs");
    assert!(
        output.status.success(),
        "shallow clone failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Repo {
        root,
        commits: source.commits.clone(),
    }
}

fn non_git_tree(source: &Repo) -> Repo {
    let root = scratch("non-git");
    for relative in [".agent-grounds/fissile.toml", "src/old_soft.rs"] {
        let content = fs::read_to_string(source.root.join(relative)).unwrap();
        write(&root, relative, &content);
    }
    Repo {
        root,
        commits: BTreeMap::new(),
    }
}

/// The command is intentionally absent when this contract lands. Every history
/// assertion is aggregated so the pre-fix run proves the missing public surface
/// without stopping before the age, rename, JSON, and refusal cases are reached.
#[test]
fn audit_history_contract() {
    let repo = build_timeline("timeline.toml");
    let mut failures = Vec::new();

    let expected_text = expand(
        &fs::read_to_string(fixture("expected-text.txt")).unwrap(),
        &repo,
    );
    let text = fissile(
        &repo.root,
        &[
            "audit",
            "--history",
            "base..to",
            "--only",
            "history",
            "--no-color",
        ],
    );
    expect_exact(&mut failures, "compact text", &text, 0, &expected_text, "");

    let default_json = fissile(&repo.root, &["audit", "--format", "json", "--no-color"]);
    if code(&default_json) != 0
        || stdout(&default_json).contains("\"history\"")
        || !stderr(&default_json).is_empty()
    {
        failures.push("default JSON changed or performed history work".to_owned());
    }
    let history_object = expand(
        fs::read_to_string(fixture("expected-history.json"))
            .unwrap()
            .trim_end(),
        &repo,
    );
    let default = stdout(&default_json);
    let expected_json = format!(
        "{},\"history\":{history_object}}}\n",
        default.trim_end().strip_suffix('}').unwrap()
    );
    let json = fissile(
        &repo.root,
        &[
            "audit",
            "--history",
            "base..to",
            "--format",
            "json",
            "--no-color",
        ],
    );
    expect_exact(&mut failures, "stable JSON", &json, 0, &expected_json, "");

    let default_text = fissile(&repo.root, &["audit", "--no-color"]);
    if code(&default_text) != 0
        || stdout(&default_text).contains("history ")
        || !stderr(&default_text).is_empty()
    {
        failures.push("default text changed or performed history work".to_owned());
    }

    let empty = fissile(
        &repo.root,
        &[
            "audit",
            "--history",
            "to..to",
            "--format",
            "json",
            "--no-color",
        ],
    );
    for empty_array in [
        "\"added\":[]",
        "\"retired\":[]",
        "\"raised\":[]",
        "\"lowered\":[]",
        "\"renamed\":[]",
    ] {
        if code(&empty) != 0 || !stdout(&empty).contains(empty_array) {
            failures.push(format!("empty movement category is not {empty_array}"));
        }
    }

    let only = fissile(&repo.root, &["audit", "--only", "history", "--no-color"]);
    expect_refusal(
        &mut failures,
        "history selection needs a range",
        &only,
        "--only history requires --history <from>..<to>",
    );
    let malformed = fissile(&repo.root, &["audit", "--history", "base...to"]);
    expect_refusal(
        &mut failures,
        "malformed range",
        &malformed,
        "history base...to: expected one <from>..<to> range",
    );
    let unresolved = fissile(&repo.root, &["audit", "--history", "missing..to"]);
    expect_refusal(
        &mut failures,
        "unresolved revision",
        &unresolved,
        "history missing..to: revision `missing` does not resolve to a commit",
    );

    add_non_ancestor(&repo);
    let non_ancestor = fissile(&repo.root, &["audit", "--history", "side..to"]);
    expect_refusal(
        &mut failures,
        "non-ancestor range",
        &non_ancestor,
        "history side..to: `side` is not an ancestor of `to`",
    );

    let plain = non_git_tree(&repo);
    let non_git = fissile(&plain.root, &["audit", "--history", "HEAD~1..HEAD"]);
    expect_refusal(
        &mut failures,
        "non-Git invocation",
        &non_git,
        "history HEAD~1..HEAD: not a Git work tree",
    );

    let shallow = shallow_clone(&repo);
    let shallow_range = format!("{}..{}", repo.commits["absent"], repo.commits["to"]);
    let unavailable = fissile(&shallow.root, &["audit", "--history", &shallow_range]);
    expect_refusal(
        &mut failures,
        "unavailable first appearance",
        &unavailable,
        "history before the shallow boundary is unavailable",
    );

    let ambiguous = build_timeline("ambiguous.toml");
    let ambiguity = fissile(&ambiguous.root, &["audit", "--history", "base..to"]);
    expect_refusal(
        &mut failures,
        "ambiguous rename evidence",
        &ambiguity,
        &format!(
            "history base..to: revision {}: ambiguous rename evidence for src/moved.rs",
            ambiguous.commits["to"]
        ),
    );

    let token = build_timeline("unsupported-token.toml");
    let evaluation = fissile(&token.root, &["audit", "--history", "base..to"]);
    expect_refusal(
        &mut failures,
        "historical evaluation failure",
        &evaluation,
        &format!(
            "history base..to: revision {}: token evaluation unavailable",
            token.commits["base"]
        ),
    );

    let check = fissile(&repo.root, &["check", "--history", "base..to"]);
    expect_refusal(
        &mut failures,
        "check remains history-free",
        &check,
        "unknown option `--history`",
    );

    let help = fissile(&repo.root, &["audit", "--help"]);
    let help_text = stdout(&help);
    if code(&help) != 0
        || !help_text.contains("--history <from>..<to>")
        || !help_text.contains("--only history")
    {
        failures.push("audit help does not publish the range and selector".to_owned());
    }

    let audit_schema = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/audit.schema.json"),
    )
    .unwrap();
    let history_schema = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema/history.schema.json"),
    )
    .unwrap();
    if !audit_schema.contains("\"history\"")
        || !audit_schema.contains("history.schema.json")
        || !history_schema.contains("\"soft_finding_ages\"")
        || !history_schema.contains("\"first_seen_commit\"")
        || !history_schema.contains("\"unevaluatedProperties\": false")
    {
        failures.push("published schemas do not close the history record shape".to_owned());
    }

    assert!(
        failures.is_empty(),
        "history contract failures:\n{}",
        failures.join("\n")
    );
}
