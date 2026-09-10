//! Review regressions for Git-DAG traversal and evidence-based rename identity
//! (§FS-004-check-audit.2.1).

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use super::{Repo, code, fissile, git_ok, scratch, stderr, stdout, write};

const CONFIG: &str = r#"fissile_config_version = 1
[scan]
include = ["src"]
exclude = []
respect_gitignore = false
[exceptions]
soft_registry = "docs/file-size-agent-exceptions.toml"
hard_registry = "docs/file-size-human-exceptions.toml"
stale = "ignore"
[[messages]]
id = "m"
text = "Split it."
[[rules]]
id = "source"
include = ["src/**/*.rs"]
unit = "lines"
soft = 1
hard = 20
message = "m"
"#;

const WORKFLOW_CONFIG: &str = r#"fissile_config_version = 1
[scan]
include = [".github"]
exclude = []
respect_gitignore = false
[exceptions]
soft_registry = "docs/file-size-agent-exceptions.toml"
hard_registry = "docs/file-size-human-exceptions.toml"
stale = "ignore"
[[messages]]
id = "m"
text = "Split it."
[[rules]]
id = "workflow"
include = [".github/**/*.yml"]
unit = "lines"
soft = 1
hard = 20
message = "m"
"#;

fn repo(label: &str, config: &str) -> Repo {
    let root = scratch(label);
    fs::create_dir_all(&root).unwrap();
    git_ok(&root, &["init", "-q", "-b", "main"]);
    write(&root, ".agent-grounds/fissile.toml", config);
    Repo {
        root,
        commits: BTreeMap::new(),
    }
}

fn commit(repo: &Repo, name: &str, date: &str) -> String {
    git_ok(&repo.root, &["add", "-A"]);
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
            name,
        ])
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    git_ok(&repo.root, &["rev-parse", "HEAD"])
}

fn exception(path: &str, rule: &str) -> String {
    format!(
        "fissile_exceptions_version = 2\n\
         [[exceptions]]\n\
         path = \"{path}\"\n\
         match = \"exact\"\n\
         rules = [\"{rule}\"]\n\
         kind = \"deferred\"\n\
         max_accepted = {{ value = 3, unit = \"lines\" }}\n\
         until = \"the file is split\"\n\
         reason = \"The file has no extraction boundary yet.\"\n"
    )
}

#[test]
fn second_parent_range_walks_the_commit_dag() {
    let repo = repo("second-parent", CONFIG);
    write(&repo.root, "src/debt.rs", "one\ntwo\n");
    let base = commit(&repo, "base", "2024-03-01T00:00:00Z");

    write(&repo.root, "main.txt", "main\n");
    commit(&repo, "main-line", "2024-03-02T00:00:00Z");
    git_ok(&repo.root, &["switch", "-q", "-c", "side", &base]);
    write(&repo.root, "side.txt", "side\n");
    let side = commit(&repo, "side-line", "2024-03-03T00:00:00Z");
    git_ok(&repo.root, &["switch", "-q", "main"]);
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
            "merge",
            "-q",
            "--no-ff",
            "side",
            "-m",
            "merge",
        ])
        .env("GIT_AUTHOR_DATE", "2024-03-05T00:00:00Z")
        .env("GIT_COMMITTER_DATE", "2024-03-05T00:00:00Z")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "merge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let merge = git_ok(&repo.root, &["rev-parse", "HEAD"]);

    let range = format!("{side}..{merge}");
    let output = fissile(
        &repo.root,
        &[
            "audit",
            "--history",
            &range,
            "--only",
            "history",
            "--no-color",
        ],
    );
    let text = stdout(&output);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
    assert!(
        text.contains(&format!("first {base} 2024-03-01T00:00:00Z; 4 days")),
        "{text}"
    );
}

#[test]
fn copied_file_is_not_rename_evidence() {
    let repo = repo("copy", CONFIG);
    write(&repo.root, "src/original.rs", "one\ntwo\n");
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exception("src/original.rs", "source"),
    );
    let base = commit(&repo, "base", "2024-04-01T00:00:00Z");
    fs::copy(
        repo.root.join("src/original.rs"),
        repo.root.join("src/copied.rs"),
    )
    .unwrap();
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exception("src/copied.rs", "source"),
    );
    let to = commit(&repo, "copy", "2024-04-03T00:00:00Z");

    let range = format!("{base}..{to}");
    let output = fissile(
        &repo.root,
        &[
            "audit",
            "--history",
            &range,
            "--format",
            "json",
            "--no-color",
        ],
    );
    let json = stdout(&output);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(json.contains("\"added\":1,\"retired\":1"), "{json}");
    assert!(json.contains("\"renamed\":0"), "{json}");
    assert!(json.contains("\"renamed\":[]"), "{json}");
}

#[test]
fn one_file_move_does_not_rewrite_its_siblings() {
    let repo = repo("one-file-move", WORKFLOW_CONFIG);
    write(&repo.root, ".github/workflows/release.yml", "one\ntwo\n");
    write(&repo.root, ".github/workflows/auto-bump.yml", "one\ntwo\n");
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exception(".github/workflows/release.yml", "workflow"),
    );
    let base = commit(&repo, "base", "2024-05-01T00:00:00Z");
    fs::create_dir_all(repo.root.join(".github/moved")).unwrap();
    fs::rename(
        repo.root.join(".github/workflows/release.yml"),
        repo.root.join(".github/moved/release.yml"),
    )
    .unwrap();
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exception(".github/moved/release.yml", "workflow"),
    );
    let to = commit(&repo, "move one", "2024-05-03T00:00:00Z");

    let range = format!("{base}..{to}");
    let output = fissile(
        &repo.root,
        &[
            "audit",
            "--history",
            &range,
            "--only",
            "history",
            "--no-color",
        ],
    );
    let text = stdout(&output);
    assert_eq!(code(&output), 0, "{}", stderr(&output));
    assert!(
        text.contains("renamed: exact-file soft docs/file-size-agent-exceptions.toml: .github/workflows/release.yml -> .github/moved/release.yml"),
        "{text}"
    );
    assert!(!text.contains("renamed: directory"), "{text}");
    assert!(
        text.contains(&format!(
            "soft finding age: .github/workflows/auto-bump.yml [rule=workflow; unit=lines] 2 > 1; first {base} 2024-05-01T00:00:00Z; 2 days"
        )),
        "{text}"
    );
}
