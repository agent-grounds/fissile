//! Config discovery is one search order shared by every command, so the
//! deprecation it reports has to reach stderr from all of them and never reach
//! stdout (§FS-001-config.8). The subject spans discovery, every command
//! surface, and the stream split, which is why it is here rather than beside
//! one module.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The line a run emits when discovery read the deprecated path
/// (§FS-001-config.8.2).
const DEPRECATED: &str =
    "fissile: warning: .agents/fissile.toml is deprecated; move it to .agent-grounds/fissile.toml";

/// The line a run emits when a config sits at both paths (§FS-001-config.8.3).
const IGNORED: &str = "fissile: warning: .agents/fissile.toml is ignored; \
                       .agent-grounds/fissile.toml is the config in effect";

const CONFIG: &str = r#"
fissile_config_version = 1
[scan]
include = ["src"]
exclude = []
respect_gitignore = false
[[messages]]
id = "m"
text = "Split {path}."
[[rules]]
id = "rust"
include = ["src/**/*.rs"]
unit = "lines"
soft = 100
hard = 200
message = "m"
"#;

/// Every command that discovers a config (§FS-001-config.8.2). `exception add`
/// stands for the family under `--dry-run`, so the surface is covered without
/// a registry write. `check` appears twice because the JSON mode is the one
/// whose stdout a caller parses.
const DISCOVERING: &[&[&str]] = &[
    &["check", "--no-color"],
    &["check", "--format", "json"],
    &["audit", "--no-color"],
    &["measure", "src/ok.rs", "--no-color"],
    &["limits", "--no-color"],
    &[
        "exception",
        "add",
        "src/big.rs",
        "--severity",
        "soft",
        "--rule",
        "rust",
        "--kind",
        "deferred",
        "--reason",
        "the parser boundary does not exist yet",
        "--until",
        "the parser module lands",
        "--dry-run",
    ],
];

/// A repository holding `CONFIG` at each named path. Passing two paths is the
/// both-present tree of §FS-001-config.8.3.
fn repo_with_config_at(paths: &[&str]) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("fissile-discovery-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    for relative in paths {
        let full = dir.join(relative);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(&full, CONFIG).unwrap();
    }
    let big: String = (0..250).map(|i| format!("fn f{i}() {{}}\n")).collect();
    fs::write(dir.join("src/big.rs"), big).unwrap();
    fs::write(dir.join("src/ok.rs"), "fn ok() {}\n").unwrap();
    dir
}

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run(root: &Path, args: &[&str]) -> Run {
    // Fenced: the fixture is outside Git, where the root is the directory the
    // run starts in, only if no work tree encloses the temp directory
    // (§FS-001-config.8.1).
    let output = Command::new(env!("CARGO_BIN_EXE_fissile"))
        .current_dir(root)
        .env("GIT_CEILING_DIRECTORIES", std::env::temp_dir())
        .args(args)
        .output()
        .expect("fissile runs");
    Run {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

/// §FS-001-config.8.2: the warning belongs to discovery, not to one command, and
/// it is one line rather than one per finding or per measured file.
#[test]
fn every_discovering_command_warns_once_on_stderr() {
    let root = repo_with_config_at(&[".agents/fissile.toml"]);
    for args in DISCOVERING {
        let run = run(&root, args);
        let count = run.stderr.matches(DEPRECATED).count();
        assert_eq!(
            count, 1,
            "fissile {args:?} put the deprecation on stderr {count} times, expected once\n\
             --- stderr ---\n{}",
            run.stderr
        );
    }
    fs::remove_dir_all(&root).unwrap();
}

/// §FS-001-config.8.2: stdout carries the findings, and under `--format json` a
/// stream a caller parses. A warning that entered it would break that caller.
#[test]
fn the_warning_never_reaches_stdout() {
    let root = repo_with_config_at(&[".agents/fissile.toml"]);
    for args in DISCOVERING {
        let run = run(&root, args);
        assert!(
            !run.stdout.contains("deprecated") && !run.stdout.contains(".agent-grounds"),
            "fissile {args:?} put the deprecation on stdout\n--- stdout ---\n{}",
            run.stdout
        );
    }

    let json = run(&root, &["check", "--format", "json"]);
    let stdout = json.stdout.trim();
    assert!(
        stdout.starts_with('[') && stdout.ends_with(']'),
        "JSON stdout is no longer one array under the warning:\n{stdout}"
    );
    fs::remove_dir_all(&root).unwrap();
}

/// §FS-001-config.8.2: a deprecated path is a warning and never a failure, so
/// the same tree exits identically from either home.
#[test]
fn a_deprecated_path_changes_no_exit_code() {
    let old = repo_with_config_at(&[".agents/fissile.toml"]);
    let new = repo_with_config_at(&[".agent-grounds/fissile.toml"]);
    for args in DISCOVERING {
        let from_old = run(&old, args);
        let from_new = run(&new, args);
        assert_eq!(
            from_old.code, from_new.code,
            "fissile {args:?} exits {} from the deprecated home and {} from the new one\n\
             --- deprecated stderr ---\n{}\n--- new stderr ---\n{}",
            from_old.code, from_new.code, from_old.stderr, from_new.stderr
        );
    }
    fs::remove_dir_all(&old).unwrap();
    fs::remove_dir_all(&new).unwrap();
}

/// §FS-001-config.8.1: an explicit `--config` is not discovery. The caller named
/// the document, so nothing is being chosen behind them and nothing is warned.
#[test]
fn an_explicit_config_never_warns() {
    let root = repo_with_config_at(&[".agents/fissile.toml"]);
    for args in DISCOVERING {
        let mut with_config: Vec<&str> = args.to_vec();
        with_config.extend(["--config", ".agents/fissile.toml"]);
        let run = run(&root, &with_config);
        assert!(
            !run.stderr.contains("deprecated"),
            "fissile {with_config:?} warned about a path the caller spelled out\n\
             --- stderr ---\n{}",
            run.stderr
        );
    }
    fs::remove_dir_all(&root).unwrap();
}

/// §FS-001-config.8.3: with both files present the new home wins, and the run
/// says which one it ignored rather than leaving the reader to find out.
#[test]
fn both_paths_present_name_the_one_ignored() {
    let root = repo_with_config_at(&[".agent-grounds/fissile.toml", ".agents/fissile.toml"]);
    for args in DISCOVERING {
        let run = run(&root, args);
        let count = run.stderr.matches(IGNORED).count();
        assert_eq!(
            count, 1,
            "fissile {args:?} said the deprecated config was ignored {count} times, expected once\n\
             --- stderr ---\n{}",
            run.stderr
        );
        assert!(
            !run.stdout.contains(IGNORED),
            "fissile {args:?} put that line on stdout\n--- stdout ---\n{}",
            run.stdout
        );
    }
    fs::remove_dir_all(&root).unwrap();
}
