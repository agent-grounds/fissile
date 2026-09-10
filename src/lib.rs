//! Core library for `fissile`.
//!
//! `fissile` keeps files small by evaluating measured files against configured
//! budgets and returning structured overflow findings with project-owned,
//! architecture-aware remediation messages.

use std::cmp::Ordering;
use std::path::Path;

pub mod audit;
pub mod check;
pub mod cli;
mod comments;
pub mod config;
pub mod entry;
pub mod entrypoint;
pub mod exception;
pub mod exceptions;
mod glob;
mod history;
pub mod hook;
pub mod init;
pub mod json;
pub mod limits;
mod loose;
mod managed;
pub mod measure;
mod model;
pub mod remove;
pub mod report;
pub mod retune;
pub mod scan;
mod staged_history;
mod toml_lines;

pub use glob::Glob;
pub use model::{
    FileMeasurement, FissileError, LineStats, Overflow, RenderedMessage, Severity, Unit,
    measure_bytes, measure_text,
};

/// A pair of optional soft and hard limits for one unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Budget {
    pub unit: Unit,
    pub soft: Option<u64>,
    pub hard: Option<u64>,
}

impl Budget {
    pub fn new(unit: Unit, soft: Option<u64>, hard: Option<u64>) -> Self {
        Self { unit, soft, hard }
    }

    fn validate(&self, rule_id: &str) -> Result<(), FissileError> {
        if self.soft.is_none() && self.hard.is_none() {
            return Err(FissileError::InvalidBudget {
                rule_id: rule_id.to_owned(),
                reason: "at least one of soft or hard must be set".to_owned(),
            });
        }

        if let (Some(soft), Some(hard)) = (self.soft, self.hard)
            && soft > hard
        {
            return Err(FissileError::InvalidBudget {
                rule_id: rule_id.to_owned(),
                reason: "soft limit cannot be greater than hard limit".to_owned(),
            });
        }

        Ok(())
    }
}

/// Which paths a rule applies to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selector {
    All,
    Extension(String),
    Prefix(String),
    Exact(String),
    /// One or more globs; the selector matches when any glob matches. This is
    /// the variant produced from a config rule's `include` list
    /// (§FS-001-config.3).
    Glob(Vec<Glob>),
}

impl Selector {
    pub fn matches(&self, path: &Path) -> bool {
        match self {
            Selector::All => true,
            Selector::Extension(extension) => path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == extension.trim_start_matches('.')),
            Selector::Prefix(prefix) => path.to_string_lossy().starts_with(prefix),
            Selector::Exact(expected) => path.to_string_lossy() == expected.as_str(),
            Selector::Glob(globs) => {
                let path = path.to_string_lossy();
                globs.iter().any(|glob| glob.matches(&path))
            }
        }
    }

    fn specificity(&self) -> (u8, usize) {
        match self {
            Selector::All => (0, 0),
            Selector::Extension(extension) => (1, extension.trim_start_matches('.').len()),
            Selector::Prefix(prefix) => (2, prefix.len()),
            Selector::Exact(path) => (3, path.len()),
            // Only consulted when a glob selector is compared against a
            // non-glob one, which a config-built checker never does; glob-vs-glob
            // overlap uses the partial order in `selector_specificity_cmp`.
            Selector::Glob(_) => (2, 0),
        }
    }
}

/// Static, project-owned guidance rendered when a rule overflows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageTemplate {
    pub id: String,
    pub text: String,
}

impl MessageTemplate {
    pub fn new(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }

    fn validate(&self, rule_id: &str) -> Result<(), FissileError> {
        if self.id.trim().is_empty() {
            return Err(FissileError::InvalidMessage {
                rule_id: rule_id.to_owned(),
                reason: "message id cannot be empty".to_owned(),
            });
        }

        if self.text.trim().is_empty() {
            return Err(FissileError::InvalidMessage {
                rule_id: rule_id.to_owned(),
                reason: "message text cannot be empty".to_owned(),
            });
        }

        Ok(())
    }

    fn render(&self, context: &MessageContext<'_>) -> RenderedMessage {
        let text = render_template(&self.text, context);

        RenderedMessage {
            id: self.id.clone(),
            text,
        }
    }
}

/// A size rule selected by path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Rule {
    pub id: String,
    pub selector: Selector,
    pub budget: Budget,
    /// Guidance for a soft overflow: what to split or record in the current
    /// commit (§DF-003-severity-guidance.1).
    pub soft_message: MessageTemplate,
    /// Guidance for a hard overflow: what has to happen before more code lands,
    /// and who to escalate to (§DF-003-severity-guidance.1).
    pub hard_message: MessageTemplate,
    pub priority: i32,
    /// Whether blank lines count toward a `lines` budget. Default `false`
    /// (§FS-001-config.3.1).
    pub count_blank_lines: bool,
    /// Whether whole-line comments count toward a `lines` budget; default `true`
    /// (§FS-001-config.3.1). Applied to the per-rule line count in [`Checker::check`].
    pub count_comment_lines: bool,
}

impl Rule {
    /// A rule whose guidance is the same at both severities. Use
    /// [`Rule::with_message`] to give one severity its own wording.
    pub fn new(
        id: impl Into<String>,
        selector: Selector,
        budget: Budget,
        message: MessageTemplate,
    ) -> Self {
        Self {
            id: id.into(),
            selector,
            budget,
            soft_message: message.clone(),
            hard_message: message,
            priority: 0,
            count_blank_lines: false,
            count_comment_lines: true,
        }
    }

    /// Override the guidance for one severity (§FS-001-config.4).
    pub fn with_message(mut self, severity: Severity, message: MessageTemplate) -> Self {
        *self.message_mut(severity) = message;
        self
    }

    /// The guidance rendered when this rule overflows at `severity`.
    pub fn message(&self, severity: Severity) -> &MessageTemplate {
        match severity {
            Severity::Soft => &self.soft_message,
            Severity::Hard => &self.hard_message,
        }
    }

    fn message_mut(&mut self, severity: Severity) -> &mut MessageTemplate {
        match severity {
            Severity::Soft => &mut self.soft_message,
            Severity::Hard => &mut self.hard_message,
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Set the line-counting policy (§FS-001-config.3.1).
    pub fn with_line_policy(mut self, count_blank_lines: bool, count_comment_lines: bool) -> Self {
        self.count_blank_lines = count_blank_lines;
        self.count_comment_lines = count_comment_lines;
        self
    }

    fn validate(&self) -> Result<(), FissileError> {
        if self.id.trim().is_empty() {
            return Err(FissileError::InvalidRule {
                reason: "rule id cannot be empty".to_owned(),
            });
        }

        self.budget.validate(&self.id)?;
        self.soft_message.validate(&self.id)?;
        self.hard_message.validate(&self.id)
    }
}

/// Evaluates configured rules against file measurements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checker {
    rules: Vec<Rule>,
    exclusions: Vec<Vec<Glob>>,
}

impl Checker {
    pub fn new(rules: Vec<Rule>) -> Result<Self, FissileError> {
        let exclusions = vec![Vec::new(); rules.len()];
        Self::with_exclusions(rules, exclusions)
    }

    /// Build the config-backed checker whose scopes may subtract rule-local
    /// exclusion globs (§FS-001-config.3.4).
    pub(crate) fn with_exclusions(
        rules: Vec<Rule>,
        exclusions: Vec<Vec<Glob>>,
    ) -> Result<Self, FissileError> {
        debug_assert_eq!(rules.len(), exclusions.len());
        for rule in &rules {
            rule.validate()?;
        }

        Ok(Self { rules, exclusions })
    }

    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Every configured rule together with its negative scope. Keeping the
    /// applicability decision on this view makes checking, measuring, audit,
    /// and inventory consume the same compiled rule (§FS-001-config.3.4).
    pub(crate) fn scoped_rules(&self) -> impl Iterator<Item = ScopedRule<'_>> {
        self.rules
            .iter()
            .zip(&self.exclusions)
            .map(|(rule, exclusions)| ScopedRule { rule, exclusions })
    }

    /// Evaluate a file and return the effective rule plus its measured value for
    /// every rule that applies. Callers that need exception logic and the
    /// hard-implies-soft rule build findings from this (§FS-004-check-audit).
    pub fn evaluate<'a>(
        &'a self,
        file: &FileMeasurement,
    ) -> Result<Vec<RuleHit<'a>>, FissileError> {
        self.effective_rules(file)?
            .into_iter()
            .map(|rule| {
                let actual = self.measured_value(file, rule)?;
                Ok(RuleHit { rule, actual })
            })
            .collect()
    }

    fn measured_value(&self, file: &FileMeasurement, rule: &Rule) -> Result<u64, FissileError> {
        let measured = match rule.budget.unit {
            Unit::Lines => file
                .lines
                .map(|stats| stats.counted(rule.count_blank_lines, rule.count_comment_lines)),
            unit => file.value(unit),
        };
        measured.ok_or_else(|| FissileError::MissingMeasurement {
            path: file.path.clone(),
            rule_id: rule.id.clone(),
            unit: rule.budget.unit,
        })
    }

    /// The commit-time view: one overflow per rule, hard suppressing soft
    /// (§GOAL-006-graded-limits). Exception registries are not consulted here.
    pub fn check(&self, file: &FileMeasurement) -> Result<Vec<Overflow>, FissileError> {
        let mut overflows = Vec::new();

        for hit in self.evaluate(file)? {
            let RuleHit { rule, actual } = hit;
            if let Some(hard) = rule.budget.hard
                && actual > hard
            {
                overflows.push(render_overflow(file, rule, Severity::Hard, actual, hard));
                continue;
            }
            if let Some(soft) = rule.budget.soft
                && actual > soft
            {
                overflows.push(render_overflow(file, rule, Severity::Soft, actual, soft));
            }
        }

        Ok(overflows)
    }

    fn effective_rules<'a>(
        &'a self,
        file: &FileMeasurement,
    ) -> Result<Vec<&'a Rule>, FissileError> {
        let mut selected: Vec<EffectiveRule<'a>> = Vec::new();

        for rule in self
            .scoped_rules()
            .filter(|scope| scope.applies_to(&file.path))
            .map(|scope| scope.rule)
        {
            match selected
                .iter()
                .position(|candidate| candidate.rule.budget.unit == rule.budget.unit)
            {
                Some(index) => match compare_rules(rule, selected[index].rule, &file.path) {
                    Ordering::Greater => {
                        selected[index] = EffectiveRule {
                            rule,
                            tied_rule_ids: Vec::new(),
                        };
                    }
                    Ordering::Equal => {
                        selected[index].tied_rule_ids.push(rule.id.clone());
                    }
                    Ordering::Less => {}
                },
                None => selected.push(EffectiveRule {
                    rule,
                    tied_rule_ids: Vec::new(),
                }),
            }
        }

        for candidate in &selected {
            if !candidate.tied_rule_ids.is_empty() {
                let mut rule_ids = Vec::with_capacity(candidate.tied_rule_ids.len() + 1);
                rule_ids.push(candidate.rule.id.clone());
                rule_ids.extend(candidate.tied_rule_ids.iter().cloned());
                return Err(FissileError::AmbiguousRules {
                    path: file.path.clone(),
                    unit: candidate.rule.budget.unit,
                    rule_ids,
                });
            }
        }

        Ok(selected
            .into_iter()
            .map(|candidate| candidate.rule)
            .collect())
    }
}

/// One compiled rule scope. Exclusions are tested against the same normalized
/// repository-relative path and glob engine as includes (§FS-001-config.3.4).
#[derive(Clone, Copy)]
pub(crate) struct ScopedRule<'a> {
    pub rule: &'a Rule,
    pub exclusions: &'a [Glob],
}

impl ScopedRule<'_> {
    pub fn applies_to(&self, path: &Path) -> bool {
        if !self.rule.selector.matches(path) {
            return false;
        }
        if self.exclusions.is_empty() {
            return true;
        }
        let path = path.to_string_lossy();
        !self.exclusions.iter().any(|glob| glob.matches(&path))
    }
}

/// An effective rule paired with the file's measured value for that rule's unit
/// (§FS-004-check-audit). Produced by [`Checker::evaluate`].
#[derive(Clone, Copy, Debug)]
pub struct RuleHit<'a> {
    pub rule: &'a Rule,
    pub actual: u64,
}

struct EffectiveRule<'a> {
    rule: &'a Rule,
    tied_rule_ids: Vec<String>,
}

fn compare_rules(left: &Rule, right: &Rule, path: &Path) -> Ordering {
    left.priority
        .cmp(&right.priority)
        .then_with(|| selector_specificity_cmp(&left.selector, &right.selector, path))
}

/// Compare two selectors' specificity for a path. Glob-vs-glob uses the glob
/// engine's partial order — incomparable globs collapse to `Equal` so the caller
/// raises an ambiguity error (§FS-001-config.3.2); other pairings use tiers.
fn selector_specificity_cmp(left: &Selector, right: &Selector, path: &Path) -> Ordering {
    match (left, right) {
        (Selector::Glob(left_globs), Selector::Glob(right_globs)) => {
            let path = path.to_string_lossy();
            match (
                best_glob_spec(left_globs, &path),
                best_glob_spec(right_globs, &path),
            ) {
                (Some(left_spec), Some(right_spec)) => left_spec.cmp_specificity(&right_spec),
                _ => Ordering::Equal,
            }
        }
        _ => left.specificity().cmp(&right.specificity()),
    }
}

/// The specificity of the most-specific glob in `globs` that matches `path`.
fn best_glob_spec(globs: &[Glob], path: &str) -> Option<glob::GlobSpec> {
    globs
        .iter()
        .filter(|glob| glob.matches(path))
        .map(Glob::spec)
        .reduce(|best, candidate| match candidate.cmp_specificity(&best) {
            Ordering::Greater => candidate,
            _ => best,
        })
}

fn render_template(template: &str, context: &MessageContext<'_>) -> String {
    let path = context.path.to_string_lossy();
    let actual = context.actual.to_string();
    let limit = context.limit.to_string();
    let unit = context.unit.to_string();

    let mut rendered = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(start) = rest.find('{') {
        rendered.push_str(&rest[..start]);
        rest = &rest[start..];

        let Some(end) = rest.find('}') else {
            rendered.push_str(rest);
            return rendered;
        };

        let placeholder = &rest[..=end];
        match placeholder {
            "{path}" => rendered.push_str(&path),
            "{rule}" => rendered.push_str(context.rule_id),
            "{severity}" => rendered.push_str(context.severity.as_str()),
            "{actual}" => rendered.push_str(&actual),
            "{limit}" => rendered.push_str(&limit),
            "{unit}" => rendered.push_str(&unit),
            _ => rendered.push_str(placeholder),
        }
        rest = &rest[end + 1..];
    }

    rendered.push_str(rest);
    rendered
}

fn render_overflow(
    file: &FileMeasurement,
    rule: &Rule,
    severity: Severity,
    actual: u64,
    limit: u64,
) -> Overflow {
    let context = MessageContext {
        path: &file.path,
        rule_id: &rule.id,
        severity,
        unit: rule.budget.unit,
        actual,
        limit,
    };

    Overflow {
        path: file.path.clone(),
        rule_id: rule.id.clone(),
        severity,
        unit: rule.budget.unit,
        actual,
        limit,
        message: rule.message(severity).render(&context),
    }
}

struct MessageContext<'a> {
    path: &'a Path,
    rule_id: &'a str,
    severity: Severity,
    unit: Unit,
    actual: u64,
    limit: u64,
}

#[cfg(test)]
#[path = "checker_tests.rs"]
mod tests;
