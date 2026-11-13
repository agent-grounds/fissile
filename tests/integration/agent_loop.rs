//! The agent-minimize loop (§GOAL-006-graded-limits.2, §GOAL-006-graded-limits.4): a soft warning carries a
//! byte-stable finding shape that an agent keys off, and shrinking the file it
//! named clears the warning on the next run. This drives the real `fissile`
//! binary through warn → shrink → clean, asserting the exact soft-finding line
//! the managed `AGENTS.md` block tells an agent to pattern-match on.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A rust rule that warns at two lines and blocks at ten, with a named message.
const CONFIG: &str = "\
fissile_config_version = 1

[scan]
include = [\"src\"]
exclude = []
respect_gitignore = false

[[messages]]
id = \"split-source\"
text = \"Split the file.\"

[[rules]]
id = \"rust\"
include = [\"src/**/*.rs\"]
unit = \"lines\"
soft = 2
hard = 10
message = \"split-source\"
";

/// The block header §GOAL-006-graded-limits.2 fixes so an agent can match without parsing
/// prose: `<severity>: <n> files over the <soft>-<unit> budget [rule: <name>,
/// message: <id>]`; a line finding names its counting basis and budget.
const SOFT_HEADER: &str = "soft: 1 file over the 2-line budget [rule: rust, message: split-source]";

/// The per-file line inside that block: `<path>: <actual> <basis> (budget <limit>)`.
const SOFT_FILE: &str = "    src/grew.rs: 5 non-blank lines (budget 2)";

fn work_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fissile-agent-loop-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join(".agent-grounds")).unwrap();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(dir.join(".agent-grounds/fissile.toml"), CONFIG).unwrap();
    dir
}

fn check(root: &Path) -> (i32, String) {
    // Fenced, so the root is this directory wherever the temp directory lives
    // (§FS-001-config.8.1).
    let output = Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(root)
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .args(["check", "--no-color"])
        .output()
        .expect("fissile runs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn soft_warning_clears_after_the_agent_shrinks_the_file() {
    let root = work_dir();
    let file = root.join("src/grew.rs");

    // The agent just grew this file to five lines, over the soft limit of two.
    fs::write(
        &file,
        "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n",
    )
    .unwrap();

    // First run: the warning is on stdout, the commit is *not* blocked (exit 0),
    // and the finding line is byte-for-byte what an agent keys off.
    let (code, stdout) = check(&root);
    assert_eq!(code, 0, "a soft overflow warns without blocking");
    assert!(
        stdout.lines().any(|line| line == SOFT_HEADER)
            && stdout.lines().any(|line| line == SOFT_FILE),
        "soft finding shape drifted; got:\n{stdout}"
    );

    // The agent follows the guidance and brings the file back under the limit.
    fs::write(&file, "fn a() {}\n").unwrap();

    // Second run: clean. The minimize loop closed without an exception entry.
    let (code, stdout) = check(&root);
    assert_eq!(code, 0);
    assert_eq!(
        stdout.trim_end(),
        "ok",
        "shrinking the file clears the warning"
    );

    let _ = fs::remove_dir_all(&root);
}
