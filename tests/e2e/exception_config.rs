//! Wrong-document guidance and unchanged registry routing for issue #110
//! (§FS-001-config.1.1, §FS-005-exception-add.1, §FS-008-exception-retune.1).
//! Ported from the reported two-file, 10-line soft-limit fixture. Hard cases
//! grow the same files past its 1000-line hard limit.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

const MAIN: &str = ".agent-grounds/fissile.toml";
const SOFT: &str = "docs/file-size-agent-exceptions.toml";
const HARD: &str = "docs/file-size-human-exceptions.toml";
const OTHER: &str = "docs/policy.toml";
const CONFIG: &str = r#"fissile_config_version = 1
[scan]
include = ["docs"]
exclude = []
respect_gitignore = false
[exceptions]
soft_registry = "docs/file-size-agent-exceptions.toml"
hard_registry = "docs/file-size-human-exceptions.toml"
[[messages]]
id = "m"
text = "Split the file."
[[rules]]
id = "citable-spec"
include = ["docs/**/*.md"]
unit = "lines"
soft = 10
hard = 1000
message = "m"
"#;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str, severity: &str) -> Self {
        let parent = std::env::var_os("FISSILE_E2E_SCRATCH").map_or_else(
            || {
                PathBuf::from(
                    std::env::var_os("HOME")
                        .or_else(|| std::env::var_os("USERPROFILE"))
                        .expect("fixture: HOME or USERPROFILE"),
                )
                .join("ag/tmp")
            },
            PathBuf::from,
        );
        let root = parent.join(format!("fissile-110-{name}-{}", std::process::id()));
        fs::create_dir_all(root.join(".agent-grounds")).expect("fixture: config directory");
        fs::create_dir_all(root.join("docs/sources")).expect("fixture: sources directory");
        let fixture = Self(root);
        let git = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&fixture.0)
            .output()
            .expect("fixture: git init runs");
        assert!(git.status.success(), "fixture: git init: {git:?}");
        fixture.write(MAIN, CONFIG);
        for path in [SOFT, HARD] {
            fixture.write(path, "fissile_exceptions_version = 2\n");
        }
        fixture.write(
            OTHER,
            "# A registry under an arbitrary filename.\n\"fissile_exceptions_version\" = 2\n",
        );
        let count = if severity == "hard" { 1020 } else { 20 };
        let lines: String = (1..=count).map(|i| format!("line {i}\n")).collect();
        for path in ["docs/sources/README.md", "docs/sources/OTHER.md"] {
            fixture.write(path, &lines);
        }
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        fs::write(self.0.join(path), text).expect("fixture: write input");
    }

    fn bytes(&self, path: &str) -> Vec<u8> {
        fs::read(self.0.join(path)).expect("fixture: read registry")
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fissile"))
            .current_dir(&self.0)
            .env("GIT_CEILING_DIRECTORIES", self.0.parent().unwrap())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(args)
            .output()
            .expect("fixture: fissile runs")
    }

    fn edit(&self, command: &str, severity: &str, path: &str, config: Option<&str>) -> Output {
        let max = match (severity, command) {
            ("soft", "add") => "30",
            ("soft", _) => "40",
            (_, "add") => "1030",
            _ => "1040",
        };
        let mut args = vec![
            "exception",
            command,
            path,
            "--severity",
            severity,
            "--rule",
            "citable-spec",
            "--max",
            max,
            "--unit",
            "lines",
        ];
        if command == "add" {
            args.extend([
                "--kind",
                "deferred",
                "--reason",
                "Missing boundary: a source index generator; split after the generator lands.",
                "--until",
                "the source index generator lands",
            ]);
            if severity == "hard" {
                args.push("--force");
            }
        }
        if let Some(config) = config {
            args.extend(["--config", config]);
        }
        self.run(&args)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn refusal(command: &str, severity: &str) {
    let fixture = Fixture::new(&format!("{command}-{severity}"), severity);
    let seed = fixture.edit("add", severity, "docs/sources/README.md", None);
    assert!(
        seed.status.success(),
        "fixture: seed existing entry: {seed:?}"
    );
    let target = if command == "add" {
        "docs/sources/OTHER.md"
    } else {
        "docs/sources/README.md"
    };
    let paths = [SOFT, HARD, OTHER, MAIN];
    let before = paths.map(|path| fixture.bytes(path));
    let mut problems = Vec::new();
    for config in [SOFT, HARD, OTHER] {
        let output = fixture.edit(command, severity, target, Some(config));
        let diagnostic = text(&output);
        let lower = diagnostic.to_lowercase();
        let recovery = [
            "drop --config",
            "omit --config",
            "without --config",
            "pass the main config",
        ]
        .iter()
        .any(|clause| lower.contains(clause));
        let expected = [
            (output.status.code() == Some(2), "exit 2"),
            (output.stdout.is_empty(), "no stdout"),
            (
                diagnostic.contains(&format!("fissile exception {command}:")),
                "command attribution",
            ),
            (diagnostic.contains(config), "supplied filename"),
            (
                lower.contains("exceptions registry")
                    && lower.contains("not")
                    && lower.contains("config"),
                "wrong-document identity",
            ),
            (
                lower.contains("--config") && lower.contains("main config"),
                "--config takes the main config",
            ),
            (diagnostic.contains(MAIN), "normal main-config path"),
            (
                diagnostic.contains("[exceptions]") && lower.contains("registries"),
                "[exceptions] registry routing",
            ),
            (recovery, "usable recovery"),
            (
                !(severity == "hard"
                    && lower.contains("soft registry")
                    && !lower.contains("hard registry")),
                "severity-neutral guidance",
            ),
            (
                !diagnostic.contains("unknown field `fissile_exceptions_version`"),
                "no generic unknown-marker error",
            ),
        ];
        let missing: Vec<_> = expected
            .into_iter()
            .filter_map(|(ok, meaning)| (!ok).then_some(meaning))
            .collect();
        if !missing.is_empty() {
            problems.push(format!(
                "{command}/{severity} --config {config}: missing {}\n  stderr: {diagnostic}",
                missing.join("; ")
            ));
        }
        for (path, bytes) in paths.iter().zip(&before) {
            assert_eq!(
                &fixture.bytes(path),
                bytes,
                "contract: refusal changed {path}"
            );
        }
    }
    assert!(
        problems.is_empty(),
        "contract: wrong-document guidance:\n{}",
        problems.join("\n")
    );
}

#[test]
fn issue_110_add_soft_identifies_a_registry_config() {
    refusal("add", "soft");
}

#[test]
fn issue_110_add_hard_identifies_a_registry_config() {
    refusal("add", "hard");
}

#[test]
fn issue_110_retune_soft_identifies_a_registry_config() {
    refusal("retune", "soft");
}

#[test]
fn issue_110_retune_hard_identifies_a_registry_config() {
    refusal("retune", "hard");
}

/// Guard, passes before the fix: discovery and explicit main configs route
/// both commands through the configured severity registry (§FS-001-config.5).
#[test]
fn issue_110_guard_discovery_and_explicit_main_config_update_the_selected_registry() {
    for severity in ["soft", "hard"] {
        for explicit in [false, true] {
            let fixture = Fixture::new(&format!("control-{severity}-{explicit}"), severity);
            let config = explicit.then_some(MAIN);
            let (selected, other) = if severity == "soft" {
                (SOFT, HARD)
            } else {
                (HARD, SOFT)
            };
            let untouched = fixture.bytes(other);
            for command in ["add", "retune"] {
                let before = fixture.bytes(selected);
                let output = fixture.edit(command, severity, "docs/sources/README.md", config);
                assert!(
                    output.status.success(),
                    "fixture: {command}/{severity} explicit={explicit}: {output:?}"
                );
                let after = fixture.bytes(selected);
                assert_ne!(before, after, "contract: {command} must update {selected}");
                let registry: toml::Value =
                    toml::from_str(std::str::from_utf8(&after).unwrap()).unwrap();
                let entries = registry["exceptions"].as_array().unwrap();
                assert_eq!(entries.len(), 1, "contract: one entry after {command}");
                assert_eq!(entries[0]["path"].as_str(), Some("docs/sources/README.md"));
                let ceiling = match (severity, command) {
                    ("soft", "add") => 30,
                    ("soft", _) => 40,
                    (_, "add") => 1030,
                    _ => 1040,
                };
                assert_eq!(
                    entries[0]["max_accepted"]["value"].as_integer(),
                    Some(ceiling)
                );
                assert_eq!(
                    fixture.bytes(other),
                    untouched,
                    "contract: wrong registry changed"
                );
            }
        }
    }
}

/// Guard, passes before the fix: registry identity is TOML structure, and
/// ordinary config diagnostics keep their detail (§FS-001-config.1.1).
#[test]
fn issue_110_guard_marker_lookalikes_keep_config_diagnostics() {
    let fixture = Fixture::new("lookalikes", "soft");
    let cases = [
        (
            "# fissile_exceptions_version = 2\nfissile_config_version = 1\n",
            None,
        ),
        (
            "fissile_config_version = 1\n[[messages]]\nid = 'm'\ntext = 'fissile_exceptions_version = 2'\n",
            None,
        ),
        (
            "fissile_config_version = 1\n[scan]\nfissile_exceptions_version = 2\n",
            Some("unknown field"),
        ),
        (
            "fissile_config_version = 1\ntypo = true\n",
            Some("unknown field `typo`"),
        ),
        (
            "# fissile_exceptions_version = 2\nfissile_config_version = 99\n",
            Some("unsupported fissile_config_version 99"),
        ),
        (
            "fissile_exceptions_version = 2\nfissile_config_version = 1\n",
            Some("unknown field"),
        ),
        (
            "fissile_exceptions_version = 2\n[broken\n",
            Some("config parse error"),
        ),
        (
            "fissile_exceptions_version = '2'\n",
            Some("config parse error"),
        ),
        (
            "fissile_exceptions_version = 999\n",
            Some("config parse error"),
        ),
    ];
    for (source, error) in cases {
        let path = "docs/lookalike-exceptions.toml";
        fixture.write(path, source);
        let output = fixture.run(&["limits", "--config", path]);
        let diagnostic = text(&output);
        assert!(
            !diagnostic.contains("main config"),
            "contract: misclassified {source:?}: {diagnostic}"
        );
        if let Some(error) = error {
            assert_eq!(
                output.status.code(),
                Some(2),
                "contract: {source:?}: {output:?}"
            );
            assert!(
                diagnostic.contains(error) && diagnostic.contains(path),
                "contract: lost {error:?} or filename for {source:?}: {diagnostic}"
            );
            if error.contains("unknown field") || source.contains("[broken") {
                assert!(
                    diagnostic.contains("line"),
                    "contract: lost error location: {diagnostic}"
                );
            }
        } else {
            assert!(
                output.status.success(),
                "contract: valid main config {source:?}: {output:?}"
            );
        }
    }
}

/// The existing flag is explained before a caller makes the mistake
/// (§FS-006-cli.2), within the existing one-screen help budget.
fn help(command: &str) {
    let fixture = Fixture::new(&format!("help-{command}"), "soft");
    let output = fixture.run(&["exception", command, "--help"]);
    assert!(output.status.success(), "contract: help exits zero");
    let raw = String::from_utf8_lossy(&output.stdout);
    let folded = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    for clause in [
        "--config",
        "main config",
        MAIN,
        "[exceptions]",
        "registries",
    ] {
        assert!(
            folded.contains(clause),
            "contract: {command} help omits {clause:?}:\n{raw}"
        );
    }
    assert!(
        raw.lines().count() <= 24,
        "contract: {command} help exceeds one screen"
    );
}

#[test]
fn issue_110_add_help_explains_the_main_config() {
    help("add");
}

#[test]
fn issue_110_retune_help_explains_the_main_config() {
    help("retune");
}
