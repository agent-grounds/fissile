//! What a defect in an exception registry says (§FS-003-exceptions.4): the site
//! it names, and the wording of each refusal. Split from the format itself
//! because a message that names the fix is its own subject
//! (§GOAL-003-friendly-output.1, §DF-007-instructions-at-the-error-site).

use std::error::Error;
use std::fmt;

use super::{INDEFINITE, Kind, SUPPORTED_VERSION};
use crate::{Severity, Unit};

/// Where one entry lives: the registry file and the entry's `path`. Diagnostics
/// lead with the pair because it is the line the reader has to edit — an entry
/// has no name to quote (§FS-003-exceptions.4, §DF-005-exception-identity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntrySite {
    pub registry: String,
    pub path: String,
}

impl fmt::Display for EntrySite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.registry, self.path)
    }
}

/// The site of one entry, for a diagnostic that has the two parts to hand.
pub(super) fn site(registry: &str, path: &str) -> EntrySite {
    EntrySite {
        registry: registry.to_owned(),
        path: path.to_owned(),
    }
}

/// A failure while loading or validating an exception registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExceptionError {
    Parse {
        registry: String,
        reason: String,
    },
    UnsupportedVersion {
        registry: String,
        version: u32,
    },
    /// Omitted and blank are one defect — the entry carries no rationale — so
    /// one variant reports both (§FS-003-exceptions.4). The severity decides
    /// whether inheriting one is a way out: only a soft entry may shadow.
    EmptyReason {
        site: EntrySite,
        severity: Severity,
    },
    EmptyUntil {
        site: EntrySite,
        severity: Severity,
    },
    KindUntilMismatch {
        site: EntrySite,
        kind: Kind,
    },
    NonPositiveMax {
        site: EntrySite,
    },
    NoRules {
        site: EntrySite,
    },
    UnknownRule {
        site: EntrySite,
        rule: String,
    },
    UnitMismatch {
        site: EntrySite,
        rule: String,
    },
    NoSeverityLimit {
        site: EntrySite,
        rule: String,
        severity: Severity,
    },
    BelowLimit {
        site: EntrySite,
        rule: String,
        max: u64,
        limit: u64,
    },
    /// More than one entry in one severity registry matches the same overflow
    /// (§FS-003-exceptions.3). `path` is the measured file; `patterns` holds the
    /// `path` values of the first two matching entries in registry order, the
    /// entries the reader has to edit (§FS-003-exceptions.4). Boxed so the pair
    /// does not grow every `Result` that carries this error.
    MultipleMatches {
        registry: String,
        path: String,
        rule: String,
        unit: Unit,
        patterns: Box<[String; 2]>,
    },
    ShadowsInHardRegistry {
        site: EntrySite,
    },
    ShadowsWithOwnRationale {
        site: EntrySite,
        field: &'static str,
    },
    ShadowsWithoutTwin {
        site: EntrySite,
        /// The hard registry's path, when there is a file to name.
        registry: Option<String>,
    },
    ShadowsAmbiguousTwin {
        site: EntrySite,
        registry: String,
        /// The two candidate entries' rule lists — the only field that can tell
        /// them apart, their paths being the same string by construction. Equal
        /// lists mean nothing does, and the message reports a duplicate instead
        /// (§FS-003-exceptions.2.3).
        rules: [String; 2],
    },
}

impl ExceptionError {
    /// Name `registry` where an absent hard registry file left the label
    /// unfilled. [`Registries::load`] is handed the text of a file that exists,
    /// so it never learns the configured path of one that does not; every
    /// caller has it from the config (§FS-003-exceptions.2.3).
    ///
    /// [`Registries::load`]: super::Registries::load
    #[must_use]
    pub fn naming_hard_registry(self, registry: &str) -> Self {
        match self {
            Self::ShadowsWithoutTwin {
                site,
                registry: None,
            } => Self::ShadowsWithoutTwin {
                site,
                registry: Some(registry.to_owned()),
            },
            other => other,
        }
    }
}

impl fmt::Display for ExceptionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExceptionError::Parse { registry, reason } => {
                write!(f, "{registry}: exception registry parse error: {reason}")
            }
            // Every adopter meets this once, on upgrade, so it names both edits
            // rather than stating a fact (§FS-003-exceptions.2.2).
            ExceptionError::UnsupportedVersion { registry, version } if *version == 1 => write!(
                f,
                "{registry}: exception registry version 1 is unsupported; this build supports {SUPPORTED_VERSION}\n\
                 to migrate this file: set fissile_exceptions_version = {SUPPORTED_VERSION}, and delete every id and replaces line — version {SUPPORTED_VERSION} removed both, because an entry is identified by the registry it lives in and what it accepts"
            ),
            ExceptionError::UnsupportedVersion { registry, version } => write!(
                f,
                "{registry}: exception registry version {version} is unsupported; this build supports {SUPPORTED_VERSION}"
            ),
            // "states no" is true of the omitted field and the blank one alike
            // (§FS-003-exceptions.4), and only a soft entry has the other way
            // out: nothing sits above a hard one (§FS-003-exceptions.2.3).
            ExceptionError::EmptyReason {
                site,
                severity: Severity::Soft,
            } => write!(
                f,
                "{site} states no reason; write one, or declare shadows = \"hard\" to inherit the hard entry's"
            ),
            ExceptionError::EmptyReason { site, .. } => {
                write!(f, "{site} states no reason")
            }
            ExceptionError::EmptyUntil {
                site,
                severity: Severity::Soft,
            } => write!(
                f,
                "{site} states no until; name what retires the entry, or declare shadows = \"hard\" to inherit the hard entry's"
            ),
            ExceptionError::EmptyUntil { site, .. } => {
                write!(f, "{site} states no until")
            }
            // The message names the distinction rather than the rule it broke:
            // the fix is usually the other kind, not a different `until`
            // (§DF-004-exception-kind.1).
            ExceptionError::KindUntilMismatch {
                site,
                kind: Kind::Structural,
            } => write!(
                f,
                "{site} is structural, so until must be \"{INDEFINITE}\"; if something would retire it, no constraint makes the split illegal and the entry is deferred"
            ),
            ExceptionError::KindUntilMismatch {
                site,
                kind: Kind::Deferred,
            } => write!(
                f,
                "{site} is deferred, so until must name what retires it, not \"{INDEFINITE}\"; use kind = \"structural\" if splitting the file is genuinely illegal"
            ),
            ExceptionError::NonPositiveMax { site } => {
                write!(f, "{site} max_accepted.value must be a positive integer")
            }
            ExceptionError::NoRules { site } => {
                write!(f, "{site} must list at least one rule id")
            }
            ExceptionError::UnknownRule { site, rule } => {
                write!(f, "{site} references unknown rule id {rule}")
            }
            ExceptionError::UnitMismatch { site, rule } => write!(
                f,
                "{site} max_accepted.unit does not match the unit of rule {rule}"
            ),
            ExceptionError::NoSeverityLimit {
                site,
                rule,
                severity,
            } => write!(
                f,
                "{site} targets rule {rule}, which has no {severity} limit to accept"
            ),
            ExceptionError::BelowLimit {
                site,
                rule,
                max,
                limit,
            } => write!(
                f,
                "{site} max_accepted.value {max} is below rule {rule} limit {limit}"
            ),
            // The file says where the collision was met, the patterns what to edit,
            // and the remedy closes it (§DF-007-instructions-at-the-error-site.1).
            ExceptionError::MultipleMatches {
                registry,
                path,
                rule,
                unit,
                patterns,
            } => {
                // Equal patterns tell the entries apart from nothing, so they are
                // named once, as `ShadowsAmbiguousTwin` does (§FS-003-exceptions.4).
                let [first, second] = &**patterns;
                write!(
                    f,
                    "{registry}: more than one exception matches {path} for {unit} rule {rule}; "
                )?;
                if first == second {
                    write!(
                        f,
                        "at least two matching entries declare path = \"{first}\"; "
                    )?;
                } else {
                    write!(
                        f,
                        "matching entries include path = \"{first}\" and path = \"{second}\"; "
                    )?;
                }
                f.write_str(
                    "remove or narrow overlapping entries so only one covers this file, rule and unit in this registry",
                )
            }
            // Each shadow error names the two ways out, because dropping the
            // pointer is as often the right one as fixing what it points at
            // (§DF-007-instructions-at-the-error-site).
            ExceptionError::ShadowsInHardRegistry { site } => write!(
                f,
                "{site} may not declare shadows: the hard registry is where a rationale lives, so there is nothing above it to point at — delete the line, or move the entry to the soft registry"
            ),
            ExceptionError::ShadowsWithOwnRationale { site, field } => write!(
                f,
                "{site} declares both shadows = \"hard\" and {field}; a shadowing entry inherits the hard entry's {field}, so a second copy here can only drift — delete {field}, or delete shadows and state this entry's own"
            ),
            ExceptionError::ShadowsWithoutTwin { site, registry } => write!(
                f,
                "{site} declares shadows = \"hard\", but {} holds no entry for this path with the same match and unit covering every rule it lists — add the hard entry whose reason and until this one would inherit, or delete shadows and state them here",
                registry.as_deref().unwrap_or("the hard registry")
            ),
            // Equal rule lists tell the candidates apart from nothing, so
            // printing both would read as a contradiction: the defect is the
            // duplicate itself, and that is what the message reports.
            ExceptionError::ShadowsAmbiguousTwin {
                site,
                registry,
                rules: [first, second],
            } if first == second => write!(
                f,
                "{site} declares shadows = \"hard\", but {registry} holds more than one entry for that address, each listing rules {first}; a shadowing entry inherits one rationale, so delete the duplicate entry there"
            ),
            ExceptionError::ShadowsAmbiguousTwin {
                site,
                registry,
                rules: [first, second],
            } => write!(
                f,
                "{site} declares shadows = \"hard\", but more than one entry in {registry} answers that address — one lists rules {first}, another {second}; a shadowing entry inherits one rationale, so remove the duplicate or list only rules a single hard entry covers"
            ),
        }
    }
}

impl Error for ExceptionError {}
