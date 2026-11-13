//! Unit tests for the invocation context (§FS-001-config.8.1): what Git's answer
//! resolves to, where the root stops, how an explicit `--config` is resolved,
//! and how a caller's path is rebased onto the root (§FS-007-measure.1).

use super::*;

use crate::scan::normalize_repo_path;

fn probe_of(top: &str, cdup: &str, prefix: &str) -> Probe {
    Probe {
        toplevel: PathBuf::from(top),
        cdup: cdup.to_owned(),
        prefix: prefix.to_owned(),
    }
}

#[test]
fn parses_one_line_per_question() {
    assert_eq!(
        parse_probe("/w/repo\n\n\n"),
        Some(probe_of("/w/repo", "", ""))
    );
    assert_eq!(
        parse_probe("/w/repo\n../../\nsub/deep/\n"),
        Some(probe_of("/w/repo", "../../", "sub/deep/"))
    );
    assert_eq!(
        parse_probe("C:/w/repo\r\n../\r\nsub/\r\n"),
        Some(probe_of("C:/w/repo", "../", "sub/"))
    );
    assert_eq!(parse_probe(""), None);
}

#[test]
fn below_the_root_the_root_is_the_way_up() {
    let below = Invocation::from_probe(Some(&probe_of("/w/repo", "../../", "sub/deep/")));
    assert_eq!(below.root(), Path::new("../.."));
    assert_eq!(below.prefix(), "sub/deep/");
}

#[test]
fn at_the_root_and_outside_git_the_root_is_the_invocation_directory() {
    let at_root = Invocation::from_probe(Some(&probe_of("/w/repo", "", "")));
    let outside = Invocation::from_probe(None);
    for invocation in [at_root, outside] {
        assert_eq!(invocation.root(), Path::new("."));
        assert_eq!(invocation.prefix(), "");
    }
}

#[test]
fn an_explicit_config_is_resolved_once_against_the_invocation_directory() {
    let top = std::env::temp_dir().join("w");
    let below = Invocation::from_probe(Some(&Probe {
        toplevel: top.clone(),
        cdup: "../../".to_owned(),
        prefix: "sub/deep/".to_owned(),
    }));
    let config = |raw: &str| below.config_path(Some(PathBuf::from(raw)));
    assert_eq!(config("local.toml"), Some(top.join("sub/deep/local.toml")));
    assert_eq!(
        config("./local.toml"),
        Some(top.join("sub/deep/local.toml"))
    );
    // `..` is folded, so `audit --history` reads it as a path in the tree.
    assert_eq!(
        config("../../.agents/fissile.toml"),
        Some(top.join(".agents/fissile.toml"))
    );
    let absolute = std::env::temp_dir().join("elsewhere.toml");
    assert_eq!(
        below.config_path(Some(absolute.clone())),
        Some(absolute.clone())
    );
    assert_eq!(below.config_path(None), None);

    // At the root and outside Git a path is kept exactly as typed.
    for invocation in [
        Invocation::from_probe(Some(&probe_of("/w/repo", "", ""))),
        Invocation::from_probe(None),
    ] {
        for raw in ["local.toml", "../x.toml", "./sub/../x.toml"] {
            assert_eq!(
                invocation.config_path(Some(PathBuf::from(raw))),
                Some(PathBuf::from(raw))
            );
        }
    }
}

#[test]
fn lexical_folds_parent_steps() {
    assert_eq!(lexical(Path::new("a/b/../c")), PathBuf::from("a/c"));
    assert_eq!(lexical(Path::new("./a/./b")), PathBuf::from("a/b"));
    assert_eq!(lexical(Path::new("a/../../b")), PathBuf::from("../b"));
    let root = std::env::temp_dir();
    let top = root.ancestors().last().unwrap().to_path_buf();
    assert_eq!(lexical(&top.join("../x")), top.join("x"));
}

#[test]
fn only_a_root_of_parent_steps_puts_the_caller_below_it() {
    for (root, levels) in [
        ("..", 1),
        ("../..", 2),
        ("../../", 2),
        (".", 0),
        ("", 0),
        ("../x", 0),
        ("x", 0),
    ] {
        assert_eq!(levels_below(Path::new(root)), levels, "{root:?}");
    }
    assert_eq!(levels_below(&std::env::temp_dir()), 0);
}

#[test]
fn a_config_document_is_spelled_from_where_the_caller_stands() {
    for (root, prefix) in [
        (".", ""),
        ("", ""),
        ("./", ""),
        ("..", "../"),
        ("../..", "../../"),
        ("../../", "../../"),
    ] {
        assert_eq!(document_prefix(Path::new(root)), prefix, "{root:?}");
    }
}

#[test]
fn no_config_is_passed_over_at_the_root() {
    // The crate root holds a config of its own; standing at it, nothing is
    // passed over, and neither is anything for a library caller's root.
    assert_eq!(passed_over(Path::new(".")), None);
    assert_eq!(passed_over(&std::env::temp_dir()), None);
}

/// The directory unit tests run in, as the kernel names it; `..` is its parent.
fn here_name() -> String {
    Path::new(".")
        .canonicalize()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

#[test]
fn a_callers_path_is_rebased_onto_the_root_below_it() {
    let here = here_name();
    let parent = Path::new("..");
    let rebased = |raw: &str| normalize_repo_path(parent, raw).unwrap();
    assert_eq!(rebased("Cargo.toml"), format!("{here}/Cargo.toml"));
    assert_eq!(rebased("./src/lib.rs"), format!("{here}/src/lib.rs"));
    assert_eq!(rebased("src/../Cargo.toml"), format!("{here}/Cargo.toml"));
    assert_eq!(rebased("."), here);
    assert_eq!(rebased(".."), crate::scan::REPOSITORY_ROOT);
    // A missing file is still an address, left for measurement to report.
    assert_eq!(
        rebased("no-such-file.rs"),
        format!("{here}/no-such-file.rs")
    );
}

#[test]
fn a_refused_path_is_named_as_the_caller_typed_it() {
    let error = normalize_repo_path(Path::new(".."), "../..").unwrap_err();
    assert_eq!(error.to_string(), "path `../..` is outside the repository");
    let error = normalize_repo_path(Path::new("."), "../").unwrap_err();
    assert_eq!(error.to_string(), "path `../` is outside the repository");
}

#[test]
fn a_library_callers_root_keeps_its_paths_relative_to_it() {
    // No `..` steps: the caller is not below this root, and nothing is rebased.
    let root = std::env::temp_dir().join("fissile-library-root");
    assert_eq!(normalize_repo_path(&root, "src/x.rs").unwrap(), "src/x.rs");
    assert_eq!(normalize_repo_path(Path::new("."), "x.py").unwrap(), "x.py");
}

/// Real repositories. Unix-only: Git names a Windows root `C:/...`, which no
/// canonical path compares with.
#[cfg(unix)]
mod git {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A scratch directory for real repositories, canonical so Git's absolute
    /// answers compare as written.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            static COUNTER: AtomicUsize = AtomicUsize::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("fissile-invocation-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir.canonicalize().unwrap())
        }

        /// Git fenced in at this directory, whatever the environment it runs in.
        fn git(&self, dir: &Path) -> Command {
            let mut command = Command::new("git");
            command
                .current_dir(dir)
                .env("GIT_CEILING_DIRECTORIES", &self.0)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env(
                    "GIT_CONFIG_GLOBAL",
                    if cfg!(windows) { "NUL" } else { "/dev/null" },
                )
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE");
            command
        }

        fn run(&self, dir: &Path, args: &[&str]) {
            let output = self.git(dir).args(args).output().unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        fn repo(&self, rel: &str) -> PathBuf {
            let root = self.0.join(rel);
            fs::create_dir_all(&root).unwrap();
            self.run(&root, &["init", "-q", "-b", "main"]);
            root
        }

        /// The probe as `probe` runs it, under the same fence.
        fn probe(&self, dir: &Path) -> Option<Probe> {
            let mut command = probe_command(dir);
            command
                .env("GIT_CEILING_DIRECTORIES", &self.0)
                .env_remove("GIT_DIR")
                .env_remove("GIT_WORK_TREE")
                .env_remove("GIT_INDEX_FILE");
            read_probe(&mut command)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_nested_repository_is_its_own_root() {
        let scratch = Scratch::new();
        scratch.repo("outer");
        let inner = scratch.repo("outer/inner");
        let sub = inner.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let probe = scratch.probe(&sub).expect("inside a work tree");
        assert_eq!(probe, probe_of(&inner.to_string_lossy(), "../", "sub/"));
        assert_eq!(Invocation::from_probe(Some(&probe)).root(), Path::new(".."));
    }

    #[test]
    fn a_linked_worktree_is_its_own_root() {
        let scratch = Scratch::new();
        let main = scratch.repo("main");
        scratch.run(
            &main,
            &[
                "-c",
                "user.name=fissile",
                "-c",
                "user.email=unit@fissile.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "first",
            ],
        );
        let linked = scratch.0.join("linked");
        let linked_arg = linked.to_string_lossy().into_owned();
        scratch.run(
            &main,
            &["worktree", "add", "-q", "-b", "linked", &linked_arg],
        );
        let deep = linked.join("a/b");
        fs::create_dir_all(&deep).unwrap();
        let probe = scratch.probe(&deep).expect("inside a work tree");
        assert_eq!(probe, probe_of(&linked.to_string_lossy(), "../../", "a/b/"));
    }

    #[test]
    fn outside_git_there_is_no_answer_and_no_ancestor_is_searched() {
        let scratch = Scratch::new();
        let plain = scratch.0.join("plain/sub");
        fs::create_dir_all(&plain).unwrap();
        let probe = scratch.probe(&plain);
        assert_eq!(probe, None);
        assert_eq!(
            Invocation::from_probe(probe.as_ref()).root(),
            Path::new(".")
        );
    }
}
