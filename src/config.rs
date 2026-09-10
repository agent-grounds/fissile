//! Versioned TOML config loading (§FS-001-config). The config is data, not code
//! (§GOAL-002-tiny-footprint); every field is optional and falls back to a
//! default, while the file `fissile init` writes is explicit (§DF-002-explicit-config).

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

mod config_document;

use crate::{Budget, Checker, FissileError, Glob, MessageTemplate, Rule, Selector, Severity, Unit};

/// The only supported major config version (§FS-001-config.1).
pub const SUPPORTED_VERSION: u32 = 1;

/// Where the config lives (§FS-001-config.8): the first path discovery looks at.
pub const CONFIG_HOME: &str = ".agent-grounds/fissile.toml";

/// The former home, still read so that no repository breaks on upgrade
/// (§FS-001-config.8, §DF-012-config-home).
pub const DEPRECATED_CONFIG_HOME: &str = ".agents/fissile.toml";

/// Said once, on stderr, by a run discovery landed on the old home
/// (§FS-001-config.8.2).
pub const DEPRECATED_WARNING: &str =
    "fissile: warning: .agents/fissile.toml is deprecated; move it to .agent-grounds/fissile.toml";

/// Said instead when a config sits at both paths: the precedence is stated
/// rather than left for the reader to discover from a rule that never fires
/// (§FS-001-config.8.3).
pub const IGNORED_WARNING: &str = "fissile: warning: .agents/fissile.toml is ignored; \
                                   .agent-grounds/fissile.toml is the config in effect";

/// Which document the effective config came from (§FS-001-config.8.1). Carried
/// out of discovery because the deprecation belongs to discovery rather than to
/// any one command, and only the command surface owns stderr
/// (§FS-001-config.8.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigSource {
    /// A path the caller named. Not discovery: the caller said which document to
    /// read, so nothing is being chosen behind them (§FS-001-config.8.1).
    Explicit(PathBuf),
    /// [`CONFIG_HOME`]. `shadows_deprecated` records that a config also sits at
    /// the old path and is therefore not read (§FS-001-config.8.3).
    Home { shadows_deprecated: bool },
    /// [`DEPRECATED_CONFIG_HOME`], read because the home is absent.
    Deprecated,
    /// No config document under the root: the built-in defaults (§FS-001-config.0).
    BuiltIn,
}

impl ConfigSource {
    /// The one warning line this run owes the reader, or `None` when the config
    /// came from somewhere that needs no comment (§FS-001-config.8.2, §8.3).
    pub fn deprecation(&self) -> Option<&'static str> {
        match self {
            ConfigSource::Deprecated => Some(DEPRECATED_WARNING),
            ConfigSource::Home {
                shadows_deprecated: true,
            } => Some(IGNORED_WARNING),
            _ => None,
        }
    }
}

/// A parsed, validated config document.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// `fissile_config_version`; must equal [`SUPPORTED_VERSION`].
    pub fissile_config_version: u32,
    #[serde(default)]
    pub scan: Scan,
    #[serde(default)]
    pub output: Output,
    #[serde(default)]
    pub exceptions: Exceptions,
    #[serde(default)]
    pub tokens: Tokens,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub rules: Vec<RuleSpec>,
}

/// `[scan]` — whole-repo audit traversal scope (§FS-001-config.2).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scan {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default = "default_true")]
    pub respect_gitignore: bool,
}

/// `[output]` — default output presentation (§FS-001-config.6).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Output {
    #[serde(default)]
    pub format: Format,
    #[serde(default)]
    pub color: Color,
    #[serde(default = "default_success")]
    pub success: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Text,
    Json,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    #[default]
    Auto,
    Always,
    Never,
}

/// `[exceptions]` — registry paths, stale handling, and the ceiling step
/// (§FS-001-config.5).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Exceptions {
    #[serde(default = "default_soft_registry")]
    pub soft_registry: String,
    #[serde(default = "default_hard_registry")]
    pub hard_registry: String,
    /// What an entry that has outlived its file costs. Read by both enforcement
    /// surfaces, so `error` fails a commit as well as an audit
    /// (§FS-001-config.5, §FS-004-check-audit.1.3).
    #[serde(default)]
    pub stale: Stale,
    #[serde(default)]
    pub bump: Bump,
}

/// `[exceptions.bump]` — the step a written ceiling is quantized to, per unit
/// (§FS-001-config.5, §DF-006-quantized-ceilings). The same step bounds the slack
/// before `audit` calls a ceiling loose (§FS-003-exceptions.7).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Bump {
    #[serde(default = "default_bump_lines")]
    pub lines: u64,
    #[serde(default = "default_bump_bytes")]
    pub bytes: u64,
    #[serde(default = "default_bump_tokens")]
    pub tokens: u64,
}

impl Bump {
    /// The step for one unit. A step of `0` or `1` means no quantization: the
    /// measurement is written exactly, and any slack at all reads as loose.
    pub fn step(&self, unit: Unit) -> u64 {
        match unit {
            Unit::Bytes => self.bytes,
            Unit::Lines => self.lines,
            Unit::Tokens => self.tokens,
        }
    }
}

impl Default for Bump {
    fn default() -> Self {
        Self {
            lines: default_bump_lines(),
            bytes: default_bump_bytes(),
            tokens: default_bump_tokens(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Stale {
    #[default]
    Warn,
    Error,
    Ignore,
}

impl Stale {
    /// Whether an entry that has outlived its file is mentioned at all.
    pub fn reports(self) -> bool {
        self != Stale::Ignore
    }

    /// Whether mentioning it also fails the run (§FS-004-check-audit.1.3).
    /// Both enforcement surfaces read the setting through these two, so a
    /// commit and an audit cannot come to different conclusions about it.
    pub fn fails(self) -> bool {
        self == Stale::Error
    }
}

/// `[tokens]` — opt-in token counting (§FS-001-config.7).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Tokens {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub command: Vec<String>,
}

/// A `[[messages]]` entry (§FS-001-config.4).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub id: String,
    pub text: String,
}

/// A `[[rules]]` entry (§FS-001-config.3).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    pub id: String,
    pub include: Vec<String>,
    /// Globs removed from this rule's scope without removing the path from any
    /// other rule (§FS-001-config.3.4).
    #[serde(default)]
    pub exclude: Vec<String>,
    pub unit: UnitSpec,
    #[serde(default)]
    pub soft: Option<u64>,
    #[serde(default)]
    pub hard: Option<u64>,
    #[serde(default)]
    pub priority: i32,
    /// Guidance for both severities unless a severity-specific field overrides
    /// it (§FS-001-config.3).
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub soft_message: Option<String>,
    #[serde(default)]
    pub hard_message: Option<String>,
    #[serde(default)]
    pub count_blank_lines: bool,
    #[serde(default = "default_true")]
    pub count_comment_lines: bool,
}

impl RuleSpec {
    /// The message ID used at `severity`: the severity-specific field when set,
    /// otherwise the shared `message` (§FS-001-config.3).
    pub fn message_id(&self, severity: Severity) -> Option<&str> {
        let specific = match severity {
            Severity::Soft => self.soft_message.as_deref(),
            Severity::Hard => self.hard_message.as_deref(),
        };
        specific.or(self.message.as_deref())
    }

    /// Whether the rule declares a threshold at `severity`, and so needs
    /// guidance for it.
    fn declares(&self, severity: Severity) -> bool {
        match severity {
            Severity::Soft => self.soft.is_some(),
            Severity::Hard => self.hard.is_some(),
        }
    }

    /// Every message ID this rule references, for coverage reporting
    /// (§FS-004-check-audit.2).
    pub fn message_ids(&self) -> impl Iterator<Item = &str> {
        [
            self.message.as_deref(),
            self.soft_message.as_deref(),
            self.hard_message.as_deref(),
        ]
        .into_iter()
        .flatten()
    }
}

/// The template for one severity. A severity the rule declares no threshold for
/// borrows the other's text — it is never rendered — so a rule that configures
/// only the half it uses stays valid.
fn resolve_message(
    spec: &RuleSpec,
    severity: Severity,
    messages: &HashMap<&str, &Message>,
) -> Result<MessageTemplate, ConfigError> {
    let id = match spec.message_id(severity) {
        Some(id) => Some(id),
        None if spec.declares(severity) => {
            return Err(ConfigError::MissingMessage {
                rule: spec.id.clone(),
                severity,
            });
        }
        // Undeclared severity: borrow the other one's template rather than
        // demanding guidance for a limit nobody set.
        None => spec.message_id(severity.other()),
    };

    // Reached only by a rule that names no message and declares no threshold;
    // `Budget::validate` rejects it for the missing threshold (§FS-001-config.3).
    let Some(id) = id else {
        return Ok(MessageTemplate::new("none", "no guidance configured"));
    };

    let message = messages
        .get(id)
        .ok_or_else(|| ConfigError::UnknownMessage {
            rule: spec.id.clone(),
            message: id.to_owned(),
        })?;
    // Trimmed so a multi-line TOML string, which a project reaches for as soon
    // as its guidance is a paragraph, does not render a blank guidance line.
    Ok(MessageTemplate::new(
        message.id.clone(),
        message.text.trim(),
    ))
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UnitSpec {
    Bytes,
    Lines,
    Tokens,
}

impl From<UnitSpec> for Unit {
    fn from(unit: UnitSpec) -> Self {
        match unit {
            UnitSpec::Bytes => Unit::Bytes,
            UnitSpec::Lines => Unit::Lines,
            UnitSpec::Tokens => Unit::Tokens,
        }
    }
}

impl Default for Scan {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: Vec::new(),
            respect_gitignore: true,
        }
    }
}

impl Default for Output {
    fn default() -> Self {
        Self {
            format: Format::default(),
            color: Color::default(),
            success: default_success(),
        }
    }
}

impl Default for Exceptions {
    fn default() -> Self {
        Self {
            soft_registry: default_soft_registry(),
            hard_registry: default_hard_registry(),
            stale: Stale::default(),
            bump: Bump::default(),
        }
    }
}

/// One hundred lines is roughly a screen of code: coarse enough that an ordinary
/// edit stays inside the ceiling it was granted, small enough that the number
/// still reads as a limit (§DF-006-quantized-ceilings.1).
fn default_bump_lines() -> u64 {
    100
}

fn default_bump_bytes() -> u64 {
    4096
}

fn default_bump_tokens() -> u64 {
    1000
}

fn default_true() -> bool {
    true
}

fn default_success() -> String {
    "ok".to_owned()
}

fn default_soft_registry() -> String {
    "docs/file-size-agent-exceptions.toml".to_owned()
}

fn default_hard_registry() -> String {
    "docs/file-size-human-exceptions.toml".to_owned()
}

/// Render a `toml` error compactly: the bare message plus `line`/`column`
/// derived from the error span, so a malformed config or registry points at the
/// offending spot without the crate's multi-line snippet (§GOAL-003-friendly-output.1).
pub(crate) fn format_toml_error(error: &toml::de::Error, source: &str) -> String {
    match error.span() {
        Some(span) => {
            let start = span.start.min(source.len());
            let mut line = 1usize;
            let mut column = 1usize;
            for &byte in &source.as_bytes()[..start] {
                if byte == b'\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
            }
            format!("{} at line {line} column {column}", error.message())
        }
        None => error.message().to_owned(),
    }
}

impl Config {
    /// Parse and validate a config document.
    pub fn parse(toml_text: &str) -> Result<Self, ConfigError> {
        parse_document(toml_text).map(|document| document.config)
    }

    /// The built-in default config (§FS-001-config.0): the same fully-populated
    /// document `fissile init` writes, used as a fallback when a repo has no
    /// config of its own.
    pub fn built_in() -> Config {
        Config::parse(crate::init::DEFAULT_CONFIG).expect("built-in default config is valid")
    }

    /// Discover and load the effective config under `root`: an `explicit` path
    /// must exist; otherwise [`CONFIG_HOME`], then [`DEPRECATED_CONFIG_HOME`],
    /// then [`Config::built_in`] (§FS-001-config.0, §FS-001-config.2,
    /// §FS-001-config.8.1). Use [`Config::discover`] where the run has to say
    /// which document it read.
    pub fn load(root: &Path, explicit: Option<&Path>) -> Result<Config, ConfigError> {
        Config::discover(root, explicit).map(|(config, _)| config)
    }

    /// [`Config::load`], reporting which document the config came from so a
    /// command can name the deprecated home it is being governed by, or the one
    /// it passed over (§FS-001-config.8.1).
    pub fn discover(
        root: &Path,
        explicit: Option<&Path>,
    ) -> Result<(Config, ConfigSource), ConfigError> {
        discover_document(root, explicit).map(|(document, source)| (document.config, source))
    }

    pub(crate) fn discover_with_soft_edit_limits(
        root: &Path,
        explicit: Option<&Path>,
    ) -> Result<(Config, ConfigSource, SoftEditLimits), ConfigError> {
        discover_document(root, explicit)
            .map(|(document, source)| (document.config, source, document.soft_edit_limits))
    }

    /// Build a [`Checker`] from the rules and messages in this config.
    pub fn to_checker(&self) -> Result<Checker, ConfigError> {
        let messages: HashMap<&str, &Message> = self
            .messages
            .iter()
            .map(|message| (message.id.as_str(), message))
            .collect();

        let mut rules = Vec::with_capacity(self.rules.len());
        let mut exclusions = Vec::with_capacity(self.rules.len());
        for spec in &self.rules {
            if spec.include.is_empty() {
                return Err(ConfigError::EmptyInclude {
                    rule: spec.id.clone(),
                });
            }

            let soft_template = resolve_message(spec, Severity::Soft, &messages)?;
            let hard_template = resolve_message(spec, Severity::Hard, &messages)?;

            let selector = Selector::Glob(spec.include.iter().map(Glob::new).collect());
            exclusions.push(spec.exclude.iter().map(Glob::new).collect());
            let budget = Budget::new(spec.unit.into(), spec.soft, spec.hard);

            rules.push(
                Rule::new(spec.id.clone(), selector, budget, hard_template)
                    .with_message(Severity::Soft, soft_template)
                    .with_priority(spec.priority)
                    .with_line_policy(spec.count_blank_lines, spec.count_comment_lines),
            );
        }

        Checker::with_exclusions(rules, exclusions).map_err(ConfigError::Engine)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SoftEditLimits(Vec<u64>);

impl SoftEditLimits {
    pub(crate) fn effective(&self, declaration: usize) -> u64 {
        self.0.get(declaration).copied().unwrap_or(5)
    }
}

struct ParsedDocument {
    config: Config,
    soft_edit_limits: SoftEditLimits,
}

fn parse_document(toml_text: &str) -> Result<ParsedDocument, ConfigError> {
    let (config, explicit_limits) = config_document::parse(toml_text)?;
    if config.fissile_config_version != SUPPORTED_VERSION {
        return Err(ConfigError::UnsupportedVersion {
            version: config.fissile_config_version,
        });
    }

    let mut limits = Vec::with_capacity(config.rules.len());
    for (index, spec) in config.rules.iter().enumerate() {
        let Some(value) = explicit_limits.get(index).and_then(Option::as_ref) else {
            limits.push(5);
            continue;
        };
        let Some(limit) = value
            .get_ref()
            .as_integer()
            .and_then(|limit| u64::try_from(limit).ok())
        else {
            return Err(invalid_soft_edit_limit(
                toml_text,
                value.span(),
                spec,
                "must be a positive integer",
            ));
        };
        if limit == 0 {
            return Err(invalid_soft_edit_limit(
                toml_text,
                value.span(),
                spec,
                "must be a positive integer",
            ));
        }
        if spec.soft.is_none() {
            return Err(invalid_soft_edit_limit(
                toml_text,
                value.span(),
                spec,
                "requires a soft limit on the same rule",
            ));
        }
        limits.push(limit);
    }
    Ok(ParsedDocument {
        config,
        soft_edit_limits: SoftEditLimits(limits),
    })
}

fn invalid_soft_edit_limit(
    source: &str,
    span: std::ops::Range<usize>,
    spec: &RuleSpec,
    reason: &str,
) -> ConfigError {
    let location = source_location(source, span.start);
    ConfigError::Parse {
        reason: format!(
            "rule {} has invalid soft_edit_limit: {reason}{location}",
            spec.id
        ),
    }
}

fn source_location(source: &str, offset: usize) -> String {
    let start = offset.min(source.len());
    let mut line = 1usize;
    let mut column = 1usize;
    for &byte in &source.as_bytes()[..start] {
        if byte == b'\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    format!(" at line {line} column {column}")
}

fn discover_document(
    root: &Path,
    explicit: Option<&Path>,
) -> Result<(ParsedDocument, ConfigSource), ConfigError> {
    if let Some(path) = explicit {
        let full = root.join(path);
        let text = fs::read_to_string(&full).map_err(|error| ConfigError::Io {
            path: full.clone(),
            reason: error.to_string(),
        })?;
        let document = parse_document(&text).map_err(|error| error.in_file(full))?;
        return Ok((document, ConfigSource::Explicit(path.to_path_buf())));
    }

    if let Some(document) = read_document_candidate(root, CONFIG_HOME)? {
        let shadows_deprecated = root.join(DEPRECATED_CONFIG_HOME).exists();
        return Ok((document, ConfigSource::Home { shadows_deprecated }));
    }
    if let Some(document) = read_document_candidate(root, DEPRECATED_CONFIG_HOME)? {
        return Ok((document, ConfigSource::Deprecated));
    }
    Ok((
        parse_document(crate::init::DEFAULT_CONFIG).expect("built-in default config is valid"),
        ConfigSource::BuiltIn,
    ))
}

fn read_document_candidate(
    root: &Path,
    relative: &str,
) -> Result<Option<ParsedDocument>, ConfigError> {
    let full = root.join(relative);
    match fs::read_to_string(&full) {
        Ok(text) => parse_document(&text)
            .map_err(|error| error.in_file(full))
            .map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ConfigError::Io {
            path: full,
            reason: error.to_string(),
        }),
    }
}

/// A failure while loading a config document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Io {
        path: PathBuf,
        reason: String,
    },
    Parse {
        reason: String,
    },
    UnsupportedVersion {
        version: u32,
    },
    EmptyInclude {
        rule: String,
    },
    UnknownMessage {
        rule: String,
        message: String,
    },
    /// A rule declares a threshold with no guidance for it (§FS-001-config.3).
    MissingMessage {
        rule: String,
        severity: Severity,
    },
    Engine(FissileError),
    /// A load-time error tagged with the document it came from
    /// (§FS-001-config.1): `Config::load` wraps, `Config::parse` stays pathless.
    InFile {
        path: PathBuf,
        error: Box<ConfigError>,
    },
}

impl ConfigError {
    fn in_file(self, path: PathBuf) -> ConfigError {
        match self {
            // Io already names its path; don't say it twice.
            error @ ConfigError::Io { .. } => error,
            error => ConfigError::InFile {
                path,
                error: Box::new(error),
            },
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io { path, reason } => {
                write!(f, "cannot read config {}: {reason}", path.display())
            }
            ConfigError::InFile { path, error } => {
                write!(f, "{}: {error}", path.display())
            }
            ConfigError::Parse { reason } => write!(f, "config parse error: {reason}"),
            ConfigError::UnsupportedVersion { version } => write!(
                f,
                "unsupported fissile_config_version {version}; this build supports version {SUPPORTED_VERSION}"
            ),
            ConfigError::EmptyInclude { rule } => {
                write!(f, "rule {rule} must list at least one include glob")
            }
            ConfigError::UnknownMessage { rule, message } => {
                write!(f, "rule {rule} references unknown message id {message}")
            }
            ConfigError::MissingMessage { rule, severity } => write!(
                f,
                "rule {rule} declares a {severity} limit with no message; set {severity}_message or message"
            ),
            ConfigError::Engine(error) => write!(f, "{error}"),
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            ConfigError::Engine(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
