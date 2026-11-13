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

#[test]
fn an_offered_path_is_unchanged_at_the_root_and_for_a_library_root() {
    let library = std::env::temp_dir().join("fissile-library-root");
    for root in [
        Path::new("."),
        Path::new(""),
        Path::new("../x"),
        library.as_path(),
    ] {
        for path in ["sub/x.py", "x.py", "other/deep/y.py"] {
            assert_eq!(spelled_from_caller(root, path), path, "{root:?} {path}");
        }
    }
}

#[test]
fn an_offered_path_is_spelled_from_where_the_caller_stands() {
    // Below the root, inside and outside the caller's own directory.
    for (prefix, path, spelled) in [
        ("sub", "sub/x.py", "x.py"),
        ("sub/", "sub/deep/x.py", "deep/x.py"),
        ("sub", "other/x.py", "../other/x.py"),
        ("sub/deep", "sub/x.py", "../x.py"),
        ("sub/deep", "other/x.py", "../../other/x.py"),
        ("sub", "subway/x.py", "../subway/x.py"),
        ("sub", "sub", "."),
        ("", "sub/x.py", "sub/x.py"),
    ] {
        assert_eq!(relative_to(prefix, path), spelled, "{prefix:?} {path}");
    }
}

#[test]
fn an_offered_path_below_the_root_runs_as_printed() {
    // The unit tests run one level below `..`: the inverse of the rebase.
    let here = here_name();
    let parent = Path::new("..");
    for path in [format!("{here}/Cargo.toml"), format!("{here}/src/lib.rs")] {
        let spelled = spelled_from_caller(parent, &path);
        assert!(!spelled.starts_with(&here), "{spelled}");
        assert_eq!(normalize_repo_path(parent, &spelled).unwrap(), path);
    }
    assert_eq!(
        spelled_from_caller(parent, "elsewhere/x.py"),
        "../elsewhere/x.py"
    );
}

fn missing() -> io::Error {
    io::Error::from(io::ErrorKind::NotFound)
}

#[test]
fn a_missing_path_below_the_root_is_folded_onto_it() {
    // The unit tests run one level below `..`, so the caller's prefix is `here`.
    let here = here_name();
    let parent = Path::new("..");
    let folded = |raw: &str| fold_missing(parent, raw, missing()).unwrap();
    // Inside the caller's directory, and out of it but still in the repository.
    assert_eq!(folded("no-such-dir/../gone.rs"), format!("{here}/gone.rs"));
    assert_eq!(folded("../other/gone.py"), "other/gone.py");
    // Through the rebase: a file-level error for measurement, not a refusal.
    assert_eq!(
        normalize_repo_path(parent, "../fissile-103-no-such/gone.py").unwrap(),
        "fissile-103-no-such/gone.py"
    );
}

#[test]
fn a_missing_path_folded_above_the_root_is_named_as_typed() {
    let error = fold_missing(Path::new(".."), "../../gone.py", missing()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "path `../../gone.py` is outside the repository"
    );
}

#[test]
fn only_a_missing_path_is_folded() {
    let denied = io::Error::new(io::ErrorKind::PermissionDenied, "../gone.py: denied");
    let error = fold_missing(Path::new(".."), "../gone.py", denied).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert_eq!(error.to_string(), "../gone.py: denied");
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
