//! One load-time validation decision for named and wildcard entries
//! (§FS-003-exceptions.4), shared by CLI loading and registry-write validation.

use std::path::Path;

use super::{Exception, ExceptionError, Matcher, Registries};
use crate::{Rule, ScopedRule, Severity, glob_overlap};

impl Registries {
    /// Wildcards bypass ID lookup alone; named rules remain strict even when
    /// their selectors cannot reach the entry (§FS-003-exceptions.4).
    pub(super) fn validate_scopes(&self, scopes: &[ScopedRule<'_>]) -> Result<(), ExceptionError> {
        for entry in self.all() {
            for id in &entry.rules {
                if id == "*" {
                    for scope in scopes {
                        if entry.can_silence(scope) {
                            entry.validate_rule(scope.rule)?;
                        }
                    }
                } else {
                    let scope = scopes
                        .iter()
                        .find(|scope| scope.rule.id == *id)
                        .ok_or_else(|| ExceptionError::UnknownRule {
                            site: entry.site(),
                            rule: id.clone(),
                        })?;
                    entry.validate_rule(scope.rule)?;
                }
            }
        }
        Ok(())
    }
}

impl Exception {
    /// Matching only silences a rule in the entry's unit (§FS-003-exceptions.3).
    /// Candidate scope is independent of today's files (§FS-003-exceptions.4).
    fn can_silence(&self, scope: &ScopedRule<'_>) -> bool {
        self.max_unit == scope.rule.budget.unit
            && match &self.matcher {
                Matcher::Exact(path) => scope.applies_to(Path::new(path)),
                Matcher::Glob(glob) => {
                    glob_overlap::reaches(glob, &scope.rule.selector, scope.exclusions)
                }
            }
    }

    /// The unit, severity and ceiling contract is identical for both entry
    /// forms and registry severities (§FS-003-exceptions.4).
    fn validate_rule(&self, rule: &Rule) -> Result<(), ExceptionError> {
        if rule.budget.unit != self.max_unit {
            return Err(ExceptionError::UnitMismatch {
                site: self.site(),
                rule: rule.id.clone(),
            });
        }
        let limit = match self.severity {
            Severity::Soft => rule.budget.soft,
            Severity::Hard => rule.budget.hard,
        }
        .ok_or_else(|| ExceptionError::NoSeverityLimit {
            site: self.site(),
            rule: rule.id.clone(),
            severity: self.severity,
        })?;
        if self.max_value < limit {
            return Err(ExceptionError::BelowLimit {
                site: self.site(),
                rule: rule.id.clone(),
                max: self.max_value,
                limit,
            });
        }
        Ok(())
    }
}
