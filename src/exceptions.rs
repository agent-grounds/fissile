//! Exception registries (§FS-003-exceptions): typed, reviewable rationales that
//! accept an oversized file. Severity comes from which registry an entry lives
//! in, not a field; each entry records the largest accepted measurement.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

use crate::config::UnitSpec;
use crate::{Glob, Rule, Severity, Unit};

/// The only supported registry version (§FS-003-exceptions.1). Version 2 removed
/// the `id` and `replaces` keys (§FS-003-exceptions.2.2).
pub const SUPPORTED_VERSION: u32 = 2;

/// Whether an entry's `path` is an exact repo-relative path or a glob.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MatchKind {
    Exact,
    Glob,
}

/// The `until` value that means "nothing retires this entry"
/// (§FS-003-exceptions.2.1).
pub const INDEFINITE: &str = "indefinite";

/// Whether an `until` value says the entry never expires. Trimmed and
/// case-insensitive, so `Indefinite` is the same value (§FS-003-exceptions.2.1).
pub fn is_indefinite(until: &str) -> bool {
    until.trim().eq_ignore_ascii_case(INDEFINITE)
}

/// Which of two questions an entry answers (§FS-003-exceptions.2.1,
/// §DF-004-exception-kind). The kind fixes what `reason` must establish and what
/// `until` may say; it is not a severity and not a priority.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An architectural constraint makes the split illegal. Never expires.
    Structural,
    /// No constraint; a boundary is missing. `until` names what retires it.
    /// The default for an entry that declares no kind.
    #[default]
    Deferred,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Structural => "structural",
            Kind::Deferred => "deferred",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a soft entry may say in place of a rationale of its own: `"hard"` points
/// at the hard entry at the same address, which carries the `reason` and the
/// `until` for both (§FS-003-exceptions.2.3).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shadows {
    Hard,
}

/// How many entries of each kind both registries hold (§FS-004-check-audit.2):
/// accepted permanently versus carrying debt someone has to retire. These are
/// entry totals, so a path present in both registries contributes twice.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KindCounts {
    pub structural: usize,
    pub deferred: usize,
}

impl KindCounts {
    /// Whether the registries hold nothing to inventory.
    pub fn is_empty(self) -> bool {
        self.structural == 0 && self.deferred == 0
    }
}

/// How many distinct literal registry path expressions carry each kind
/// (§FS-004-check-audit.2). A path present in both registries is counted once,
/// and structural takes precedence over deferred.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KindPathCounts {
    pub structural: usize,
    pub deferred: usize,
}

impl KindPathCounts {
    /// Whether the registries hold no path expressions to inventory.
    pub fn is_empty(self) -> bool {
        self.structural == 0 && self.deferred == 0
    }
}

/// `max_accepted = { value, unit }` — the ceiling this entry accepts.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MaxAccepted {
    pub value: u64,
    pub unit: UnitSpec,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    fissile_exceptions_version: u32,
    #[serde(default)]
    exceptions: Vec<RawException>,
}

/// Just the version line, read leniently. A version-1 registry fails the strict
/// parse on its `id` keys before the version is ever looked at, and the version
/// is the error that leads to the fix (§FS-003-exceptions.2.2).
#[derive(Debug, Deserialize)]
struct RegistryHeader {
    fissile_exceptions_version: u32,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawException {
    path: String,
    #[serde(rename = "match")]
    match_kind: MatchKind,
    rules: Vec<String>,
    max_accepted: MaxAccepted,
    /// Required, except on a shadowing entry, which has them filled in from its
    /// hard twin before the entry is built (§FS-003-exceptions.2.3).
    #[serde(default)]
    until: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    /// Optional: an entry that omits it reads as `Deferred`, and the `until`
    /// agreement is not checked (§FS-003-exceptions.2.1).
    #[serde(default)]
    kind: Option<Kind>,
    /// Soft-registry only: this entry's rationale lives in the hard entry at
    /// the same address (§FS-003-exceptions.2.3).
    #[serde(default)]
    shadows: Option<Shadows>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    issue: Option<String>,
}

/// A parsed, structurally-valid exception entry with a compiled path matcher.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exception {
    /// The registry file this entry came from, so a diagnostic can name the
    /// file whose line the reader edits (§DF-005-exception-identity).
    pub registry: String,
    pub severity: Severity,
    pub path: String,
    pub match_kind: MatchKind,
    pub rules: Vec<String>,
    pub max_value: u64,
    pub max_unit: Unit,
    pub until: String,
    /// Resolved kind: an undeclared one reads as [`Kind::Deferred`]
    /// (§FS-003-exceptions.2.1).
    pub kind: Kind,
    pub reason: String,
    pub title: Option<String>,
    pub owner: Option<String>,
    pub issue: Option<String>,
    matcher: Matcher,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Matcher {
    Exact(String),
    Glob(Glob),
}

impl Exception {
    /// Where a diagnostic about this entry points (§FS-003-exceptions.4).
    pub fn site(&self) -> EntrySite {
        EntrySite {
            registry: self.registry.clone(),
            path: self.path.clone(),
        }
    }

    /// Whether the entry's `["*"]` wildcard or explicit list covers `rule_id`.
    pub fn applies_to_rule(&self, rule_id: &str) -> bool {
        self.rules.iter().any(|r| r == "*" || r == rule_id)
    }

    /// Whether the entry's path matcher covers a repo-relative `/`-path.
    pub fn matches_path(&self, path: &str) -> bool {
        match &self.matcher {
            Matcher::Exact(expected) => expected == path,
            Matcher::Glob(glob) => glob.matches(path),
        }
    }
}

/// How an overflow relates to the exception registry of its severity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// No entry matches the `(path, rule, unit)` condition.
    None,
    /// An entry matches and the measurement is within its accepted ceiling.
    Silenced(&'a Exception),
    /// An entry matches but the file grew past its ceiling, so the finding stands.
    Exceeded(&'a Exception),
}

/// One registry document to load: where it lives and what it says. The path is
/// what diagnostics name, because it is the file whose line a reader has to edit
/// (§DF-005-exception-identity).
#[derive(Clone, Copy, Debug)]
pub struct RegistrySource<'a> {
    pub path: &'a str,
    pub text: &'a str,
}

impl<'a> RegistrySource<'a> {
    pub fn new(path: &'a str, text: &'a str) -> Self {
        Self { path, text }
    }
}

/// Both severity registries, loaded and structurally validated.
#[derive(Clone, Debug, Default)]
pub struct Registries {
    pub soft: Vec<Exception>,
    pub hard: Vec<Exception>,
}

impl Registries {
    /// Parse the soft and hard registry documents. A `None` source means the
    /// registry file is absent, which is treated as empty.
    ///
    /// Both documents are read before either is built: a shadowing soft entry
    /// takes its `reason` and `until` off the hard entry at the same address,
    /// so neither is known until the hard registry has been read
    /// (§FS-003-exceptions.2.3).
    pub fn load(
        soft: Option<RegistrySource<'_>>,
        hard: Option<RegistrySource<'_>>,
    ) -> Result<Self, ExceptionError> {
        let mut soft_raw = parse_raw(soft)?;
        let hard_raw = parse_raw(hard)?;
        // A rationale lives in the hard registry, so nothing there has anything
        // above it to point at (§FS-003-exceptions.2.3).
        if let Some(entry) = hard_raw.iter().find(|entry| entry.shadows.is_some()) {
            return Err(ExceptionError::ShadowsInHardRegistry {
                site: site(registry_path(hard), &entry.path),
            });
        }
        // The hard registry is built first, so a defect in an entry a twin
        // points at is reported where it lives, not at the twin inheriting it.
        let hard_entries = build_all(hard_raw, Severity::Hard, hard)?;
        resolve_shadows(
            &mut soft_raw,
            &hard_entries,
            registry_path(soft),
            hard,
            false,
        )?;
        Ok(Self {
            soft: build_all(soft_raw, Severity::Soft, soft)?,
            hard: hard_entries,
        })
    }

    /// Every entry across both registries.
    pub fn all(&self) -> impl Iterator<Item = &Exception> {
        self.soft.iter().chain(self.hard.iter())
    }

    fn registry(&self, severity: Severity) -> &[Exception] {
        match severity {
            Severity::Soft => &self.soft,
            Severity::Hard => &self.hard,
        }
    }

    /// Validate every entry against the configured rules (§FS-003-exceptions.4):
    /// listed rules exist, share the entry's unit, carry the relevant severity
    /// limit, and the accepted ceiling is at least that limit.
    pub fn validate_against(&self, rules: &[Rule]) -> Result<(), ExceptionError> {
        for entry in self.all() {
            // Mixed-unit rule lists are rejected for explicit lists too: a single
            // entry may only silence rules that share its unit (§FS-003-exceptions.3).
            for rule_id in &entry.rules {
                if rule_id == "*" {
                    continue;
                }
                let Some(rule) = rules.iter().find(|r| &r.id == rule_id) else {
                    return Err(ExceptionError::UnknownRule {
                        site: entry.site(),
                        rule: rule_id.clone(),
                    });
                };
                if rule.budget.unit != entry.max_unit {
                    return Err(ExceptionError::UnitMismatch {
                        site: entry.site(),
                        rule: rule_id.clone(),
                    });
                }
                let limit = match entry.severity {
                    Severity::Soft => rule.budget.soft,
                    Severity::Hard => rule.budget.hard,
                };
                let Some(limit) = limit else {
                    return Err(ExceptionError::NoSeverityLimit {
                        site: entry.site(),
                        rule: rule_id.clone(),
                        severity: entry.severity,
                    });
                };
                if entry.max_value < limit {
                    return Err(ExceptionError::BelowLimit {
                        site: entry.site(),
                        rule: rule_id.clone(),
                        max: entry.max_value,
                        limit,
                    });
                }
            }
        }
        Ok(())
    }

    /// Resolve how the severity-matching registry treats one overflow. Returns a
    /// schema error when more than one entry matches the same `(path, rule, unit)`
    /// condition (§FS-003-exceptions.3).
    pub fn verdict(
        &self,
        severity: Severity,
        path: &str,
        rule_id: &str,
        unit: Unit,
        actual: u64,
    ) -> Result<Verdict<'_>, ExceptionError> {
        let mut matched: Option<&Exception> = None;
        for entry in self.registry(severity) {
            if entry.max_unit == unit && entry.applies_to_rule(rule_id) && entry.matches_path(path)
            {
                if let Some(first) = matched {
                    // The first two matches in registry order are the entries to
                    // edit, so the refusal names them (§FS-003-exceptions.3).
                    return Err(ExceptionError::MultipleMatches {
                        registry: entry.registry.clone(),
                        path: path.to_owned(),
                        rule: rule_id.to_owned(),
                        unit,
                        patterns: Box::new([first.path.clone(), entry.path.clone()]),
                    });
                }
                matched = Some(entry);
            }
        }

        Ok(match matched {
            None => Verdict::None,
            Some(entry) if actual <= entry.max_value => Verdict::Silenced(entry),
            Some(entry) => Verdict::Exceeded(entry),
        })
    }

    /// Entries per kind across both registries (§FS-004-check-audit.2): how many
    /// entries are accepted permanently, and how many carry debt to retire. Soft and
    /// hard entries for the same path each contribute, so such a twin counts twice.
    pub fn kind_counts(&self) -> KindCounts {
        let mut counts = KindCounts::default();
        for entry in self.all() {
            match entry.kind {
                Kind::Structural => counts.structural += 1,
                Kind::Deferred => counts.deferred += 1,
            }
        }
        counts
    }

    /// Distinct literal registry paths per kind across both registries
    /// (§FS-004-check-audit.2). Duplicate path strings, including duplicate
    /// glob expressions, count once without expanding them; a structural entry
    /// wins for a path regardless of iteration order.
    pub fn kind_path_counts(&self) -> KindPathCounts {
        let mut paths = HashMap::new();
        for entry in self.all() {
            paths
                .entry(entry.path.as_str())
                .and_modify(|structural| *structural |= entry.kind == Kind::Structural)
                .or_insert(entry.kind == Kind::Structural);
        }

        let structural = paths.values().filter(|&&structural| structural).count();
        KindPathCounts {
            structural,
            deferred: paths.len() - structural,
        }
    }

    /// Entries whose path/glob matches none of `scanned` (§FS-004-check-audit.2).
    /// The one staleness answer both commands read: `audit` takes all of them,
    /// `check` the exact-path ones (§FS-004-check-audit.1.3).
    pub fn stale<'a>(&'a self, scanned: &[String]) -> Vec<&'a Exception> {
        self.all()
            .filter(|entry| !scanned.iter().any(|path| entry.matches_path(path)))
            .collect()
    }
}

fn parse_raw(source: Option<RegistrySource<'_>>) -> Result<Vec<RawException>, ExceptionError> {
    let Some(source) = source else {
        return Ok(Vec::new());
    };
    let unsupported = |version| ExceptionError::UnsupportedVersion {
        registry: source.path.to_owned(),
        version,
    };
    let file: RegistryFile = match toml::from_str(source.text) {
        Ok(file) => file,
        // The version outranks whatever else the parse tripped on: an
        // unmigrated registry fails on a key the fix removes anyway, and the
        // version error is the one that names the fix (§FS-003-exceptions.2.2).
        Err(error) => {
            return Err(match declared_version(source.text) {
                Some(version) if version != SUPPORTED_VERSION => unsupported(version),
                _ => ExceptionError::Parse {
                    registry: source.path.to_owned(),
                    reason: crate::config::format_toml_error(&error, source.text),
                },
            });
        }
    };

    if file.fissile_exceptions_version != SUPPORTED_VERSION {
        return Err(unsupported(file.fissile_exceptions_version));
    }

    Ok(file.exceptions)
}

/// The configured path of one registry, for a diagnostic to name. An absent
/// file holds no entries, so the empty label it yields never reaches a message.
fn registry_path(source: Option<RegistrySource<'_>>) -> &str {
    source.map_or("", |source| source.path)
}

fn build_all(
    raw: Vec<RawException>,
    severity: Severity,
    source: Option<RegistrySource<'_>>,
) -> Result<Vec<Exception>, ExceptionError> {
    raw.into_iter()
        .map(|raw| build_exception(raw, severity, registry_path(source)))
        .collect()
}

/// Give every `shadows = "hard"` soft entry the `reason` and `until` of the one
/// hard entry at its address, and refuse the shapes that would make the pointer
/// meaningless (§FS-003-exceptions.2.3).
fn resolve_shadows(
    soft: &mut [RawException],
    hard: &[Exception],
    soft_registry: &str,
    hard_source: Option<RegistrySource<'_>>,
    tolerate_missing: bool,
) -> Result<Vec<usize>, ExceptionError> {
    let mut orphaned = Vec::new();
    for (index, entry) in soft
        .iter_mut()
        .enumerate()
        .filter(|(_, entry)| entry.shadows.is_some())
    {
        // An entry with a rationale of its own is not shadowing one, and a
        // second copy of either field is the drift the pointer removes.
        for (field, stated) in [
            ("reason", entry.reason.is_some()),
            ("until", entry.until.is_some()),
        ] {
            if stated {
                return Err(ExceptionError::ShadowsWithOwnRationale {
                    site: site(soft_registry, &entry.path),
                    field,
                });
            }
        }
        let unit = entry.max_accepted.unit.into();
        let twins = shadow_twins(hard, &entry.path, entry.match_kind, unit, &entry.rules);
        let twin = match twins.as_slice() {
            [twin] => *twin,
            [] if tolerate_missing => {
                orphaned.push(index);
                continue;
            }
            [] => {
                return Err(ExceptionError::ShadowsWithoutTwin {
                    site: site(soft_registry, &entry.path),
                    registry: hard_source.map(|source| source.path.to_owned()),
                });
            }
            [first, second, ..] => {
                return Err(ExceptionError::ShadowsAmbiguousTwin {
                    site: site(soft_registry, &entry.path),
                    registry: registry_path(hard_source).to_owned(),
                    rules: [rule_list(first), rule_list(second)],
                });
            }
        };
        entry.reason = Some(twin.reason.clone());
        entry.until = Some(twin.until.clone());
    }
    Ok(orphaned)
}

/// The hard entries a `shadows = "hard"` soft entry at this address inherits
/// from (§FS-003-exceptions.2.3): the same `path` spelling, the same `match`,
/// the same unit, and `rules` covering every rule the twin lists. Exactly one
/// of them is a legal registry, and `exception add` holds `--shadows-hard` to
/// the same count before it writes (§FS-005-exception-add.1.1).
pub fn shadow_twins<'a>(
    hard: &'a [Exception],
    path: &str,
    match_kind: MatchKind,
    unit: Unit,
    rules: &[String],
) -> Vec<&'a Exception> {
    hard.iter()
        .filter(|entry| {
            entry.path == path
                && entry.match_kind == match_kind
                && entry.max_unit == unit
                && rules.iter().all(|rule| entry.applies_to_rule(rule))
        })
        .collect()
}

/// One entry's rule list, as a diagnostic spells it apart from another's.
fn rule_list(entry: &Exception) -> String {
    format!("[{}]", entry.rules.join(", "))
}

/// The declared version of a document the strict parse rejected, when it has one.
fn declared_version(text: &str) -> Option<u32> {
    toml::from_str::<RegistryHeader>(text)
        .ok()
        .map(|header| header.fissile_exceptions_version)
}

fn build_exception(
    mut raw: RawException,
    severity: Severity,
    registry: &str,
) -> Result<Exception, ExceptionError> {
    // Absent and blank are one defect — the entry carries no rationale — and a
    // shadowing entry has its twin's filled in by now (§FS-003-exceptions.2.3).
    let reason = raw.reason.take().unwrap_or_default();
    let until = raw.until.take().unwrap_or_default();
    // An entry has no name, so every diagnostic locates it: registry file plus
    // the entry's own `path` (§DF-005-exception-identity).
    let site = || site(registry, &raw.path);
    if reason.trim().is_empty() {
        return Err(ExceptionError::EmptyReason {
            site: site(),
            severity,
        });
    }
    if until.trim().is_empty() {
        return Err(ExceptionError::EmptyUntil {
            site: site(),
            severity,
        });
    }
    if raw.max_accepted.value == 0 {
        return Err(ExceptionError::NonPositiveMax { site: site() });
    }
    if raw.rules.is_empty() {
        return Err(ExceptionError::NoRules { site: site() });
    }
    // Checked only when the entry declares a kind, so an entry that omits one
    // keeps loading (§FS-003-exceptions.2.1). A shadowing entry is checked here
    // too, against the `until` it inherited (§FS-003-exceptions.2.3).
    if let Some(kind) = raw.kind
        && (kind == Kind::Structural) != is_indefinite(&until)
    {
        return Err(ExceptionError::KindUntilMismatch { site: site(), kind });
    }

    let matcher = match raw.match_kind {
        MatchKind::Exact => Matcher::Exact(raw.path.clone()),
        MatchKind::Glob => Matcher::Glob(Glob::new(raw.path.clone())),
    };

    Ok(Exception {
        registry: registry.to_owned(),
        severity,
        path: raw.path,
        match_kind: raw.match_kind,
        rules: raw.rules,
        max_value: raw.max_accepted.value,
        max_unit: raw.max_accepted.unit.into(),
        until,
        kind: raw.kind.unwrap_or_default(),
        reason,
        title: raw.title,
        owner: raw.owner,
        issue: raw.issue,
        matcher,
    })
}

// The diagnostics live beside the format they report on, in their own file:
// what a registry may say and how a defect in it is worded are two subjects,
// and the wording is the part §GOAL-003-friendly-output holds to a standard.
#[path = "exceptions_errors.rs"]
mod errors;
use errors::site;
pub use errors::{EntrySite, ExceptionError};

#[path = "exceptions_removal.rs"]
mod removal;
pub(crate) use removal::RemovalEntry;

#[cfg(test)]
#[path = "exceptions_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "exceptions_collision_tests.rs"]
mod collision_tests;
