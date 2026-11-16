//! `fissile init` — install config, exception registries, and the managed agent
//! block, fully populated at their defaults (§FS-002-init, §DF-002-explicit-config).
//! Project-owned files are never overwritten.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::managed::Block;

/// The fully-populated starter config written by `init` (§DF-002-explicit-config).
/// Its two markdown rules are the two reading modes: a citable spec is fetched
/// by section, an entrypoint is loaded whole (§FS-001-config.0.1).
pub const DEFAULT_CONFIG: &str = include_str!("templates/fissile.default.toml");

/// Starter soft (agent) exception registry (§FS-003-exceptions).
pub const DEFAULT_SOFT_REGISTRY: &str = include_str!("templates/soft-exceptions.toml");

/// Starter hard (human) exception registry (§FS-003-exceptions).
pub const DEFAULT_HARD_REGISTRY: &str = include_str!("templates/hard-exceptions.toml");

/// The canonical managed agent-instruction block, markers included
/// (§FS-002-init.4).
pub const MANAGED_BLOCK: &str = include_str!("templates/agents-block.md");

const BLOCK_BEGIN: &str = "<!-- BEGIN FISSILE MANAGED BLOCK -->";
const BLOCK_END: &str = "<!-- END FISSILE MANAGED BLOCK -->";
/// The block's own heading, which states its version and — in a v1 or v2 file,
/// written before the markers existed — was its only boundary (§FS-002-init.4).
const BLOCK_HEADING: &str = "## Keeping Files Small With fissile (v";
/// v4 names which hook runs `--staged` (§FS-002-init.4); v1–v3 upgrade in place.
const SUPPORTED_BLOCK_VERSION: u32 = 4;

/// How `init` finds, versions, and rewrites the agent block (§FS-002-init.4).
fn agent_block() -> Block<'static> {
    Block {
        begin_prefix: BLOCK_BEGIN,
        end_prefix: BLOCK_END,
        version: SUPPORTED_BLOCK_VERSION,
        body: MANAGED_BLOCK.trim_end(),
        version_heading: Some(BLOCK_HEADING),
    }
}

/// Which agent entrypoint families to write (§FS-002-init.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentTargets {
    pub agents_md: bool,
    pub claude: bool,
    pub gemini: bool,
    pub copilot: bool,
    pub cursor: bool,
    pub windsurf: bool,
    pub zed: bool,
}

impl AgentTargets {
    fn any(&self) -> bool {
        self.agents_md
            || self.claude
            || self.gemini
            || self.copilot
            || self.cursor
            || self.windsurf
            || self.zed
    }

    /// Entrypoint files requested by explicit flags, relative to the repo root.
    fn explicit_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if self.agents_md {
            paths.push(PathBuf::from("AGENTS.md"));
        }
        if self.claude {
            paths.push(PathBuf::from("CLAUDE.md"));
        }
        if self.gemini {
            paths.push(PathBuf::from("GEMINI.md"));
        }
        if self.copilot {
            paths.push(PathBuf::from(".github/copilot-instructions.md"));
        }
        if self.cursor {
            paths.push(PathBuf::from(".cursor/rules/fissile.mdc"));
        }
        if self.windsurf {
            paths.push(PathBuf::from(".windsurfrules"));
        }
        if self.zed {
            paths.push(PathBuf::from(".rules"));
        }
        paths
    }
}

/// Whether `init` installs the managed pre-commit hook (§FS-002-init.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HookMode {
    /// Install when the target is a git repository; skip silently otherwise.
    #[default]
    Auto,
    /// Always install; error when the target is not a git repository (`--hook`).
    Always,
    /// Never install (`--no-hook`).
    Never,
}

/// The hook step 2 of the `next:` block has to report (§FS-002-init.5): what a
/// run leaves behind, not what its flags asked for (§FS-002-init.6). No
/// `Default` — the only defensible one is the variant that promises a hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookStatus {
    /// The managed block is in the hook: this run wrote, appended, or refreshed
    /// it, it was already current, or `--no-hook` left an earlier run's in
    /// place. Under `--dry-run`, the one the run would write (§FS-002-init.6).
    Installed,
    /// Automatic mode found no git repository, so there is no hook to run.
    SkippedNotGit,
    /// `--no-hook` declined the install and no managed block was there already.
    SkippedByFlag,
}

/// Inputs to an `init` run (§FS-002-init.1).
#[derive(Clone, Debug)]
pub struct InitOptions {
    pub root: PathBuf,
    pub config_path: PathBuf,
    /// Project name for a freshly created `AGENTS.md` heading; defaults to the
    /// target directory basename (§FS-002-init.1).
    pub name: Option<String>,
    pub exceptions: bool,
    pub force: bool,
    pub dry_run: bool,
    pub agents: AgentTargets,
    /// Pre-commit hook install policy (§FS-002-init.6).
    pub hook: HookMode,
}

impl InitOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            config_path: PathBuf::from(crate::config::CONFIG_HOME),
            name: None,
            exceptions: false,
            force: false,
            dry_run: false,
            agents: AgentTargets::default(),
            hook: HookMode::Auto,
        }
    }
}

/// What `init` did to one file (§FS-002-init.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Wrote,
    Appended,
    Updated,
    Exists,
    /// Pointed at `AGENTS.md` (§FS-002-init.3, §DF-009-one-file-agents-read).
    Linked,
    /// Left a regular file: it holds bytes of its own, or the filesystem
    /// refused a link, so the block was written into it (§FS-002-init.3).
    Kept,
}

impl Action {
    fn prefix(self, dry_run: bool) -> &'static str {
        match (self, dry_run) {
            (Action::Wrote, false) => "wrote",
            (Action::Wrote, true) => "would-write",
            (Action::Appended, false) => "appended",
            (Action::Appended, true) => "would-append",
            (Action::Updated, false) => "updated",
            (Action::Updated, true) => "would-update",
            (Action::Exists, _) => "exists",
            (Action::Linked, false) => "linked",
            (Action::Linked, true) => "would-link",
            (Action::Kept, false) => "kept",
            (Action::Kept, true) => "would-keep",
        }
    }

    pub(crate) fn changed(self) -> bool {
        !matches!(self, Action::Exists)
    }
}

/// One reported path and what happened to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub path: PathBuf,
    pub action: Action,
}

/// The full result of an `init` run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub outcomes: Vec<Outcome>,
    /// The config this run wrote or found. Step 1 of the `next:` block names it
    /// rather than a literal, so it cannot send the reader to a file the run did
    /// not touch (§FS-002-init.5).
    pub config: PathBuf,
    pub dry_run: bool,
    /// The hook this run leaves behind (§FS-002-init.6); the `next:` block must
    /// not promise one that is not there (§FS-002-init.5).
    pub hook: HookStatus,
    /// Agent entrypoints this run touched, in reported order (§FS-002-init.3).
    /// The `next:` block names one of these rather than a fixed filename, so it
    /// cannot point at a file that was never written (§FS-002-init.5).
    pub entrypoints: Vec<PathBuf>,
    /// The deprecation this run owes the reader (§FS-001-config.8.2). `init`
    /// does not discover, so it is set only where it deferred to a config at
    /// the old home: a `--config` the caller spelled out is their choice, and
    /// nothing is being decided behind them (§FS-001-config.8.1).
    pub deprecation: Option<&'static str>,
}

impl Report {
    /// Whether the run changed anything; drives the `next:` block (§FS-002-init.5).
    pub fn changed_anything(&self) -> bool {
        self.outcomes.iter().any(|outcome| outcome.action.changed())
    }

    /// Render the per-path report lines plus an optional `next:` block.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        for outcome in &self.outcomes {
            lines.push(format!(
                "{} {}",
                outcome.action.prefix(self.dry_run),
                outcome.path.display()
            ));
        }
        if self.changed_anything() {
            let hook_step = match self.hook {
                HookStatus::Installed => NEXT_HOOK_STEP,
                HookStatus::SkippedNotGit => NEXT_HOOK_STEP_NO_GIT,
                HookStatus::SkippedByFlag => NEXT_HOOK_STEP_NO_HOOK,
            };
            let config = self.config.display();
            let mut block = format!(
                "next:\n\
                 1. Review {config}: the source rule budgets common code extensions; \
                 add this repo's languages or tune the limits.\n\
                 2. {hook_step}\n\
                 3. Run fissile audit once and add justified exceptions with fissile exception add."
            );
            // Omitted rather than invented when no entrypoint was written
            // (§FS-002-init.5).
            if let Some(entrypoint) = self.entrypoints.first() {
                block.push_str(&format!(
                    "\nsee {} for what agents are told; the findings carry the rest.",
                    entrypoint.display()
                ));
            }
            lines.push(block);
        }
        lines.join("\n")
    }
}

const NEXT_HOOK_STEP: &str =
    "Commit a change to see the pre-commit hook run fissile check --staged.";
const NEXT_HOOK_STEP_NO_GIT: &str = "Run git init && fissile init to install the pre-commit hook, \
    or wire fissile check --staged into your commit flow.";
/// The flag is the reason, so the step names it and points at the commit flow.
/// `init` never looked for a hook manager, so it does not say the repo has one
/// (§FS-002-init.5).
const NEXT_HOOK_STEP_NO_HOOK: &str = "--no-hook skipped the managed hook; wire fissile check \
    --staged into your commit flow — a hook manager or core.hooksPath, if this repo uses one.";

/// A failure during `init`.
#[derive(Debug)]
pub enum InitError {
    Io(io::Error),
    /// `None` when the block declares no version this build can read: a later
    /// generation renamed the heading that carries it (§FS-002-init.4).
    UnsupportedBlock {
        path: PathBuf,
        version: Option<u32>,
    },
    NotAGitRepo {
        root: PathBuf,
    },
}

impl std::fmt::Display for InitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InitError::Io(error) => write!(f, "{error}"),
            InitError::UnsupportedBlock {
                path,
                version: Some(version),
            } => write!(
                f,
                "{} has an unsupported managed block version v{version}; this build writes v{SUPPORTED_BLOCK_VERSION}",
                path.display()
            ),
            InitError::UnsupportedBlock {
                path,
                version: None,
            } => write!(
                f,
                "{} has a managed block that declares no version this build can read; this build writes v{SUPPORTED_BLOCK_VERSION}",
                path.display()
            ),
            InitError::NotAGitRepo { root } => write!(
                f,
                "{} is not a git repository; cannot install the pre-commit hook",
                root.display()
            ),
        }
    }
}

impl std::error::Error for InitError {}

impl From<io::Error> for InitError {
    fn from(error: io::Error) -> Self {
        InitError::Io(error)
    }
}

/// Run `init` against the filesystem.
pub fn run(options: &InitOptions) -> Result<Report, InitError> {
    let mut outcomes = Vec::new();

    // 1. Config — written when absent, never overwritten (§FS-002-init.2).
    let (config_path, deprecated) = existing_config(options);
    outcomes.push(write_new_file(
        &options.root.join(&config_path),
        DEFAULT_CONFIG,
        options.dry_run,
    )?);

    // 2. Exception registries, only with --exceptions (§FS-002-init.2). The
    //    paths come from the generated config so they stay in lockstep with it.
    if options.exceptions {
        let config = crate::config::Config::built_in();
        outcomes.push(write_new_file(
            &options.root.join(&config.exceptions.soft_registry),
            DEFAULT_SOFT_REGISTRY,
            options.dry_run,
        )?);
        outcomes.push(write_new_file(
            &options.root.join(&config.exceptions.hard_registry),
            DEFAULT_HARD_REGISTRY,
            options.dry_run,
        )?);
    }

    // 3. Agent entrypoints and managed blocks (§FS-002-init.3).
    let name = project_name(options);
    let mut entrypoints = Vec::new();
    // The block has to live in a real file and every link points at it, so the
    // canonical entrypoint is always in the set (§FS-002-init.3).
    let canonical = PathBuf::from(crate::entrypoint::CANONICAL);
    let mut companions = resolve_entrypoints(&options.root, &options.agents);
    companions.retain(|path| path != &canonical);

    // Planned first: both answers read bytes that writing the block changes
    // (§FS-002-init.3).
    let plans = crate::entrypoint::plan(&options.root, &companions, options.dry_run)?;

    outcomes.push(write_managed_block(
        &options.root.join(&canonical),
        &name,
        options.dry_run,
    )?);
    entrypoints.push(canonical);

    for (relative, plan) in companions.into_iter().zip(plans) {
        outcomes.push(settle(options, &relative, plan, &name)?);
        entrypoints.push(relative);
    }

    // 4. Managed pre-commit hook (§FS-002-init.6). Automatic mode installs only
    //    inside a git repo; `--hook` forces it; `--no-hook` opts out. Every arm
    //    yields a status, so a later mode cannot inherit one it never chose.
    let is_git_repo = crate::hook::is_git_repo(&options.root);
    let hook = match options.hook {
        HookMode::Always | HookMode::Auto if is_git_repo => {
            outcomes.push(crate::hook::install(&options.root, options.dry_run)?);
            HookStatus::Installed
        }
        // A guarded arm does not answer for its variant, so each mode keeps an
        // unguarded one: `--hook` outside a repository errors instead.
        HookMode::Always => {
            return Err(InitError::NotAGitRepo {
                root: options.root.clone(),
            });
        }
        HookMode::Auto => HookStatus::SkippedNotGit,
        // The flag declines the install; it does not remove the hook an earlier
        // run installed, and step 2 reports the file (§FS-002-init.5).
        HookMode::Never if crate::hook::is_installed(&options.root) => HookStatus::Installed,
        HookMode::Never => HookStatus::SkippedByFlag,
    };

    Ok(Report {
        outcomes,
        config: config_path,
        dry_run: options.dry_run,
        hook,
        entrypoints,
        deprecation: deprecated.then_some(crate::config::DEPRECATED_WARNING),
    })
}

/// Which config document this run is about, and whether it deferred to the
/// deprecated home to get there (§FS-002-init.2). A repository whose config is
/// still at the old path has an existing config, so the default home defers to
/// it: writing the generated default at the new path instead would take
/// precedence over the project's own rules on the very next run
/// (§FS-001-config.8.1). The rule is about the default path, not about how the
/// caller arrived at it: a `--config` spelling out that same path defers too,
/// because the shadowing this prevents does not care who named the file. Any
/// other `--config` is taken as given, and never remarked on.
fn existing_config(options: &InitOptions) -> (PathBuf, bool) {
    let deprecated = PathBuf::from(crate::config::DEPRECATED_CONFIG_HOME);
    if options.config_path == Path::new(crate::config::CONFIG_HOME)
        && !options.root.join(&options.config_path).exists()
        && options.root.join(&deprecated).exists()
    {
        return (deprecated, true);
    }
    (options.config_path.clone(), false)
}

/// The project name for a fresh `AGENTS.md` heading: the `--name` value, else the
/// target directory basename (§FS-002-init.1).
fn project_name(options: &InitOptions) -> String {
    if let Some(name) = &options.name {
        return name.clone();
    }
    options
        .root
        .canonicalize()
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .unwrap_or_else(|| "project".to_owned())
}

fn is_agents_md(path: &Path) -> bool {
    crate::entrypoint::is_canonical(path)
}

/// Decide which entrypoint files to touch (§FS-002-init.3).
fn resolve_entrypoints(root: &Path, agents: &AgentTargets) -> Vec<PathBuf> {
    if agents.any() {
        return agents.explicit_paths();
    }

    // Automatic mode: update known existing entrypoints; if none exist, fall
    // back to the canonical AGENTS.md.
    const KNOWN: &[&str] = &[
        "AGENTS.md",
        "AGENTS.override.md",
        "CLAUDE.md",
        ".claude/CLAUDE.md",
        "GEMINI.md",
        ".github/copilot-instructions.md",
        ".cursor/rules/fissile.mdc",
        ".cursorrules",
        ".windsurfrules",
        ".rules",
    ];

    let mut paths: Vec<PathBuf> = KNOWN
        .iter()
        .map(PathBuf::from)
        .filter(|relative| root.join(relative).exists())
        .collect();

    // Workspace-triggered aliases: create when the tool's directory exists.
    for (dir, entry) in [
        (".claude", ".claude/CLAUDE.md"),
        (".gemini", "GEMINI.md"),
        (".cursor", ".cursor/rules/fissile.mdc"),
        (".zed", ".rules"),
    ] {
        let entry = PathBuf::from(entry);
        if root.join(dir).is_dir() && !paths.contains(&entry) {
            paths.push(entry);
        }
    }

    if paths.is_empty() {
        paths.push(PathBuf::from("AGENTS.md"));
    }
    paths
}

/// Write a file only if absent; report `exists` otherwise (§FS-002-init.2).
fn write_new_file(path: &Path, contents: &str, dry_run: bool) -> Result<Outcome, InitError> {
    if path.exists() {
        return Ok(Outcome {
            path: path.to_path_buf(),
            action: Action::Exists,
        });
    }
    if !dry_run {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    Ok(Outcome {
        path: path.to_path_buf(),
        action: Action::Wrote,
    })
}

/// Carry out one companion's plan: a link to `AGENTS.md`, or the file it
/// already was with the block written in (§FS-002-init.3).
fn settle(
    options: &InitOptions,
    relative: &Path,
    plan: crate::entrypoint::Plan,
    name: &str,
) -> Result<Outcome, InitError> {
    use crate::entrypoint::Plan;
    let path = options.root.join(relative);

    let linked = match plan {
        Plan::Linked => {
            return Ok(Outcome {
                path,
                action: Action::Exists,
            });
        }
        Plan::Link => crate::entrypoint::write_link(&options.root, relative, options.dry_run)?,
        Plan::Keep => false,
    };

    if linked {
        return Ok(Outcome {
            path,
            action: Action::Linked,
        });
    }
    // Bytes of its own, or a filesystem that refused the link.
    let written = write_managed_block(&path, name, options.dry_run)?;
    Ok(Outcome {
        action: Action::Kept,
        ..written
    })
}

/// Append, replace, or leave the managed block in an entrypoint (§FS-002-init.4).
fn write_managed_block(path: &Path, name: &str, dry_run: bool) -> Result<Outcome, InitError> {
    let existing = match fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };

    let (new_contents, action) = match existing {
        // A fresh canonical AGENTS.md gets an unmanaged project H1 above the
        // block; companion entrypoints are block-only (§FS-002-init.4).
        None if is_agents_md(path) => (
            format!("# {name}\n\n{}\n", MANAGED_BLOCK.trim_end()),
            Action::Wrote,
        ),
        None => (format!("{}\n", MANAGED_BLOCK.trim_end()), Action::Wrote),
        Some(existing) => apply_managed_block(&existing, path)?,
    };

    if action.changed() && !dry_run {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &new_contents)?;
    }

    Ok(Outcome {
        path: path.to_path_buf(),
        action,
    })
}

/// Compute the new file content after applying the managed block to existing
/// text. Returns the content and the action taken (§FS-002-init.4).
fn apply_managed_block(existing: &str, path: &Path) -> Result<(String, Action), InitError> {
    agent_block().apply(existing, path)
}

#[cfg(test)]
#[path = "init_tests.rs"]
mod tests;
