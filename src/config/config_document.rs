//! Private config-document shape for command-only rule metadata.
//!
//! Deserializing this shape directly from the source keeps serde's byte spans
//! intact while leaving the public [`RuleSpec`] struct compatible with callers
//! that construct it literally.

use serde::Deserialize;

use super::{
    CONFIG_HOME, Color, Config, ConfigError, Exceptions, Format, Message, Output, RuleSpec, Scan,
    Tokens, UnitSpec, default_success, default_true, format_toml_error,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    fissile_config_version: u32,
    #[serde(default)]
    scan: Scan,
    #[serde(default)]
    output: DocumentOutput,
    #[serde(default)]
    exceptions: Exceptions,
    #[serde(default)]
    tokens: Tokens,
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    rules: Vec<DocumentRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentOutput {
    #[serde(default)]
    format: Format,
    #[serde(default)]
    color: Color,
    #[serde(default = "default_success")]
    success: String,
}

impl Default for DocumentOutput {
    fn default() -> Self {
        Self {
            format: Format::default(),
            color: Color::default(),
            success: default_success(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentRule {
    id: String,
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
    unit: UnitSpec,
    #[serde(default)]
    soft: Option<u64>,
    #[serde(default)]
    hard: Option<u64>,
    #[serde(default)]
    priority: i32,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    soft_message: Option<String>,
    #[serde(default)]
    hard_message: Option<String>,
    #[serde(default)]
    count_blank_lines: bool,
    #[serde(default = "default_true")]
    count_comment_lines: bool,
    #[serde(default)]
    soft_edit_limit: Option<toml::Spanned<toml::Value>>,
}

pub(super) type SpannedLimit = Option<toml::Spanned<toml::Value>>;

/// Parse the main config, explaining a registry supplied in its place (§FS-001-config.1.1).
pub(super) fn parse(source: &str) -> Result<(Config, Vec<SpannedLimit>), ConfigError> {
    let document: Document = toml::from_str(source).map_err(|error| ConfigError::Parse {
        reason: if has_registry_identity(source) {
            format!(
                "this document is an exceptions registry, not a fissile config.\n  \
                 --config takes the main config (normally {CONFIG_HOME}), whose \
                 [exceptions] table names the severity-specific registries.\n  \
                 Omit --config to use normal discovery, or pass the main config."
            )
        } else {
            format_toml_error(&error, source)
        },
    })?;
    let mut limits = Vec::with_capacity(document.rules.len());
    let rules = document
        .rules
        .into_iter()
        .map(|rule| {
            limits.push(rule.soft_edit_limit);
            RuleSpec {
                id: rule.id,
                include: rule.include,
                exclude: rule.exclude,
                unit: rule.unit,
                soft: rule.soft,
                hard: rule.hard,
                priority: rule.priority,
                message: rule.message,
                soft_message: rule.soft_message,
                hard_message: rule.hard_message,
                count_blank_lines: rule.count_blank_lines,
                count_comment_lines: rule.count_comment_lines,
            }
        })
        .collect();
    Ok((
        Config {
            fissile_config_version: document.fissile_config_version,
            scan: document.scan,
            output: Output {
                format: document.output.format,
                color: document.output.color,
                success: document.output.success,
            },
            exceptions: document.exceptions,
            tokens: document.tokens,
            messages: document.messages,
            rules,
        },
        limits,
    ))
}

/// Recognize only an unambiguous supported registry identity in valid TOML (§FS-001-config.1.1).
/// This checks document identity, not the schema of the registry's entries. It runs only
/// after main-config deserialization fails, preserving its original error and byte spans.
fn has_registry_identity(source: &str) -> bool {
    toml::from_str::<toml::Table>(source).is_ok_and(|document| {
        !document.contains_key("fissile_config_version")
            && document
                .get("fissile_exceptions_version")
                .and_then(toml::Value::as_integer)
                == Some(i64::from(crate::exceptions::SUPPORTED_VERSION))
    })
}
