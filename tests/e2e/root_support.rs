//! Fixtures for running the real binary from below a repository's root
//! (§FS-001-config.8.1). Each scenario builds Git repositories in a scratch
//! directory and compares a run from the root with the same run from a
//! subdirectory. Every child process is fenced in with `GIT_CEILING_DIRECTORIES`.
//! The scratch filesystem may itself sit inside a work tree, so without the fence
//! a directory meant to be outside Git would be inside that tree.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The rule of the reported case: `**/*.py` over 3 non-blank lines is hard
/// (issue #103).
pub const PY_CONFIG: &str = r#"fissile_config_version = 1

[[messages]]
id = "split"
text = "Split it."

[[rules]]
id = "py"
include = ["**/*.py"]
unit = "lines"
soft = 2
hard = 3
message = "split"
"#;

/// Four non-blank lines: one over the reported hard limit of 3.
pub const FOUR_LINES: &str = "a = 1\nb = 2\nc = 3\nd = 4\n";

/// A scratch directory under `~/ag/tmp` (or `FISSILE_E2E_SCRATCH`), canonical
/// so the absolute paths a note prints compare as written, and removed on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        let parent = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || {
                let home = std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .expect("HOME or USERPROFILE names the scratch filesystem");
                PathBuf::from(home).join("ag/tmp")
            },
            PathBuf::from,
        );
        let dir = parent.join(format!("fissile-103-e2e-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch directory");
        Self(fs::canonicalize(&dir).expect("canonicalize scratch directory"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// A fresh repository on `main` at `<scratch>/<name>`.
    pub fn repo(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        fs::create_dir_all(&root).unwrap();
        git(&self.0, &root, &["init", "-q", "-b", "main"]);
        root
    }

    /// Run the real binary in `dir`, with Git fenced in at this scratch directory.
    pub fn fissile(&self, dir: &Path, args: &[&str]) -> Ran {
        let output = Command::new(env!("CARGO_BIN_EXE_fissile"))
            .current_dir(dir)
            .env("GIT_CEILING_DIRECTORIES", &self.0)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(args)
            .output()
            .expect("fissile runs");
        Ran::new(dir, args, &output)
    }

    /// Run git in `dir` under the same fence, asserting success.
    pub fn git(&self, dir: &Path, args: &[&str]) -> String {
        git(&self.0, dir, args)
    }

    /// Stage everything and commit it.
    pub fn commit(&self, root: &Path, message: &str) {
        self.git(root, &["add", "-A"]);
        self.git(root, &["commit", "-qm", message]);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(ceiling: &Path, dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .env("GIT_CEILING_DIRECTORIES", ceiling)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_AUTHOR_NAME", "fissile-103")
        .env("GIT_AUTHOR_EMAIL", "e2e@fissile.invalid")
        .env("GIT_COMMITTER_NAME", "fissile-103")
        .env("GIT_COMMITTER_EMAIL", "e2e@fissile.invalid")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=e2e-no-hooks",
        ])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned()
}

/// Write `text` to `root/rel`, creating parent directories.
pub fn write(root: &Path, rel: &str, text: &str) {
    let target = root.join(rel);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, text).unwrap();
}

/// `count` distinct non-blank lines.
pub fn lines(count: usize, salt: &str) -> String {
    (0..count).map(|i| format!("{salt}_{i} = {i}\n")).collect()
}

/// One finished run: what it was, where, and what it said.
pub struct Ran {
    pub dir: PathBuf,
    pub args: Vec<String>,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Ran {
    fn new(dir: &Path, args: &[&str], output: &Output) -> Self {
        Self {
            dir: dir.to_path_buf(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// The whole run, for a failure message.
    pub fn report(&self) -> String {
        format!(
            "fissile {} (in {})\n  exit: {:?}\n  stdout:\n{}\n  stderr:\n{}",
            self.args.join(" "),
            self.dir.display(),
            self.code,
            self.stdout,
            self.stderr
        )
    }
}

/// `got` must give the same verdict as `want`: the same stdout bytes and the
/// same exit status. Returns the mismatch, if any, so a scenario can collect
/// every one before failing.
pub fn same_verdict(want: &Ran, got: &Ran) -> Option<String> {
    (want.stdout != got.stdout || want.code != got.code).then(|| {
        format!(
            "expected the verdict of\n{}\nbut got\n{}",
            want.report(),
            got.report()
        )
    })
}

/// Fail with every collected problem at once.
pub fn assert_none(problems: &[String], what: &str) {
    assert!(problems.is_empty(), "{what}:\n\n{}", problems.join("\n\n"));
}
