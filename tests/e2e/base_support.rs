//! Real-Git fixtures shared by the `check --base <ref>` scenarios
//! (§FS-004-check-audit.1.5). A base run reads commits, so every fixture builds its
//! history as Git objects before the real binary runs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::git;

/// `docs/**/*.md` is over soft above 20 lines; `[scan].include` names only
/// `docs`, which a base run must not consult (§FS-001-config.2).
pub const CONFIG: &str = r#"
fissile_config_version = 1

[scan]
include = ["docs"]
exclude = [".git/**", "docs/generated/**"]
respect_gitignore = true

[output]
format = "text"
color = "never"
success = "ok"

[exceptions]
soft_registry = "docs/file-size-agent-exceptions.toml"
hard_registry = "docs/file-size-human-exceptions.toml"
stale = "error"

[[messages]]
id = "split-spec"
text = "Split now or record the soft debt."

[[rules]]
id = "citable-spec"
include = ["**/*.md"]
unit = "lines"
soft = 20
hard = 200
count_blank_lines = false
count_comment_lines = true
message = "split-spec"
"#;

/// A scratch repository under `~/ag/tmp` (or `FISSILE_E2E_SCRATCH`), removed on drop.
pub struct Work(pub PathBuf);

impl Work {
    pub fn new(name: &str) -> Self {
        let scratch = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .expect("HOME or USERPROFILE names the scratch filesystem");
                PathBuf::from(home).join("ag/tmp")
            },
            PathBuf::from,
        );
        let root = scratch.join(format!("fissile-base-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("create base fixture");
        Self(root)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Commit everything, with the developer's own Git configuration neutralized.
pub fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(
        root,
        &[
            "-c",
            "user.email=e2e@fissile.invalid",
            "-c",
            "user.name=e2e",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=e2e-no-hooks",
            "commit",
            "-qm",
            message,
        ],
    );
}

/// A repository on `main` holding only the config.
pub fn init(name: &str) -> Work {
    let work = Work::new(name);
    let root = work.path();
    fs::create_dir_all(root.join(".agent-grounds")).unwrap();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join(".agent-grounds/fissile.toml"), CONFIG).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    commit(root, "config");
    work
}

/// Write `lines` distinct non-blank lines to a repo-relative path.
pub fn write_lines(root: &Path, path: &str, lines: usize, salt: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    let body: String = (0..lines)
        .map(|i| format!("Sentence {i} of {salt}.\n"))
        .collect();
    fs::write(target, body).unwrap();
}

/// Commit `docs/spec.md` at `lines` lines.
pub fn edit_spec(root: &Path, lines: usize, message: &str) {
    write_lines(root, "docs/spec.md", lines, message);
    commit(root, message);
}

/// `main` carries `before` over-soft edits of `docs/spec.md`; `HEAD` is a new
/// `topic` branch at `main`.
pub fn branch_after(name: &str, before: usize) -> Work {
    let work = init(name);
    for edit in 0..before {
        edit_spec(work.path(), 30 + edit, &format!("main edit {}", edit + 1));
    }
    git(work.path(), &["checkout", "-q", "-b", "topic"]);
    work
}

pub fn fissile(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("fissile runs")
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// Whitespace folded to single spaces, so a remedy is asserted however it wraps.
pub fn folded(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The one JSON finding for `path`, from a `--format json` run.
pub fn json_finding(output: &Output, path: &str) -> serde_json::Value {
    let records: serde_json::Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("JSON stdout ({error}): {output:?}"));
    let mut matching = records
        .as_array()
        .expect("a findings array")
        .iter()
        .filter(|record| record["path"] == path)
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "one finding for {path}: {records}");
    matching.remove(0)
}
