//! Round-two regressions for historical provenance and identity evidence
//! (§FS-004-check-audit.2.1).

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use super::{Repo, code, expect_refusal, fissile, git_ok, scratch, stderr, stdout, write};

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

fn exception_body(path: &str) -> String {
    format!(
        "[[exceptions]]\n\
         path = \"{path}\"\n\
         match = \"exact\"\n\
         rules = [\"source\"]\n\
         kind = \"deferred\"\n\
         max_accepted = {{ value = 3, unit = \"lines\" }}\n\
         until = \"the file is split\"\n\
         reason = \"The file has no extraction boundary yet.\"\n"
    )
}

fn exceptions(paths: &[&str]) -> String {
    let mut text = "fissile_exceptions_version = 2\n".to_owned();
    for path in paths {
        text.push_str(&exception_body(path));
    }
    text
}

fn merge_without_committing(repo: &Repo, branch: &str) {
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
            "--no-commit",
            branch,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "merge failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_shallow_endpoint_cannot_claim_first_appearance() {
    let source = repo("shallow-source", CONFIG);
    write(&source.root, "src/debt.rs", "one\ntwo\n");
    commit(&source, "base", "2024-06-01T00:00:00Z");
    write(&source.root, "README.md", "endpoint\n");
    commit(&source, "endpoint", "2024-06-03T00:00:00Z");

    let root = scratch("depth-one");
    let url = format!("file://{}", source.root.display());
    let output = Command::new("git")
        .args(["clone", "-q", "--depth", "1", &url])
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let shallow = Repo {
        root,
        commits: BTreeMap::new(),
    };
    assert_eq!(
        git_ok(&shallow.root, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );

    let output = fissile(
        &shallow.root,
        &[
            "audit",
            "--history",
            "HEAD..HEAD",
            "--only",
            "history",
            "--no-color",
        ],
    );
    let mut failures = Vec::new();
    expect_refusal(
        &mut failures,
        "depth-one first appearance",
        &output,
        "history before the shallow boundary is unavailable",
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn merge_age_restarts_when_one_parent_lacks_the_state() {
    let repo = repo("merge-absence", CONFIG);
    write(&repo.root, "src/deferred.rs", "one\ntwo\n");
    write(&repo.root, "src/finding.rs", "one\ntwo\n");
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exceptions(&["src/deferred.rs"]),
    );
    let base = commit(&repo, "base", "2024-06-01T00:00:00Z");

    write(&repo.root, "README.md", "main\n");
    commit(&repo, "main", "2024-06-02T00:00:00Z");
    git_ok(&repo.root, &["switch", "-q", "-c", "side", &base]);
    fs::remove_file(repo.root.join("src/deferred.rs")).unwrap();
    fs::remove_file(repo.root.join("src/finding.rs")).unwrap();
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        "fissile_exceptions_version = 2\n",
    );
    commit(&repo, "side absent", "2024-06-03T00:00:00Z");
    git_ok(&repo.root, &["switch", "-q", "main"]);
    merge_without_committing(&repo, "side");
    write(&repo.root, "src/deferred.rs", "one\ntwo\n");
    write(&repo.root, "src/finding.rs", "one\ntwo\n");
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exceptions(&["src/deferred.rs"]),
    );
    let merge = commit(&repo, "merge restored", "2024-06-05T00:00:00Z");

    let range = format!("{base}..{merge}");
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
    let deferred = format!(
        concat!(
            "deferred age: soft docs/file-size-agent-exceptions.toml: src/deferred.rs ",
            "[match=exact; rules=source; unit=lines] = 3; first {} ",
            "2024-06-05T00:00:00Z; 0 days"
        ),
        merge
    );
    assert!(text.contains(&deferred), "{text}");
    let finding = format!(
        concat!(
            "soft finding age: src/finding.rs [rule=source; unit=lines] 2 > 1; ",
            "first {} 2024-06-05T00:00:00Z; 0 days"
        ),
        merge
    );
    assert!(text.contains(&finding), "{text}");
}

#[test]
fn outside_tree_relative_config_is_refused() {
    let repo = repo("external-config", CONFIG);
    write(&repo.root, "src/debt.rs", "one\ntwo\n");
    let base = commit(&repo, "base", "2024-07-01T00:00:00Z");
    let to = commit_after_change(&repo, "to", "2024-07-03T00:00:00Z");
    let name = format!(
        "{}-outside.toml",
        repo.root.file_name().unwrap().to_string_lossy()
    );
    let external = repo.root.parent().unwrap().join(&name);
    fs::write(&external, CONFIG).unwrap();
    let argument = format!("../{name}");
    let range = format!("{base}..{to}");
    let output = fissile(
        &repo.root,
        &[
            "audit",
            "--config",
            &argument,
            "--history",
            &range,
            "--only",
            "history",
            "--no-color",
        ],
    );
    fs::remove_file(external).unwrap();
    let mut failures = Vec::new();
    expect_refusal(
        &mut failures,
        "outside-tree config",
        &output,
        "configured file is outside the committed tree",
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn commit_after_change(repo: &Repo, name: &str, date: &str) -> String {
    write(&repo.root, "README.md", &format!("{name}\n"));
    commit(repo, name, date)
}

fn directory_config(prefix: &str) -> String {
    CONFIG
        .replace("include = [\"src\"]", &format!("include = [\"{prefix}\"]"))
        .replace("src/**/*.rs", &format!("{prefix}/**/*.rs"))
}

#[test]
fn partial_registry_substitution_does_not_promote_a_directory() {
    let repo = repo("partial-directory", &directory_config("old"));
    write(&repo.root, "old/a.rs", "alpha\none\n");
    write(&repo.root, "old/b.rs", "beta\ntwo\n");
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exceptions(&["old/a.rs", "old/b.rs"]),
    );
    let base = commit(&repo, "base", "2024-08-01T00:00:00Z");

    fs::create_dir_all(repo.root.join("new")).unwrap();
    fs::rename(repo.root.join("old/a.rs"), repo.root.join("new/a.rs")).unwrap();
    fs::rename(repo.root.join("old/b.rs"), repo.root.join("new/b.rs")).unwrap();
    write(
        &repo.root,
        ".agent-grounds/fissile.toml",
        &directory_config("new"),
    );
    write(
        &repo.root,
        "docs/file-size-agent-exceptions.toml",
        &exceptions(&["new/a.rs", "old/b.rs"]),
    );
    let to = commit(&repo, "partial move", "2024-08-03T00:00:00Z");

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
    assert!(!text.contains("renamed: directory"), "{text}");
    assert!(
        text.contains(concat!(
            "renamed: exact-file soft docs/file-size-agent-exceptions.toml: ",
            "old/a.rs -> new/a.rs"
        )),
        "{text}"
    );
    let unchanged = format!(
        concat!(
            "deferred age: soft docs/file-size-agent-exceptions.toml: old/b.rs ",
            "[match=exact; rules=source; unit=lines] = 3; first {} ",
            "2024-08-01T00:00:00Z; 2 days"
        ),
        base
    );
    assert!(text.contains(&unchanged), "{text}");
    assert!(!text.contains("added: soft docs/file-size-agent-exceptions.toml: old/b.rs"));
    assert!(!text.contains("retired: soft docs/file-size-agent-exceptions.toml: old/b.rs"));
}
