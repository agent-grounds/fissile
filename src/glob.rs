//! Minimal glob engine for config rule selectors (§FS-001-config.3). `**` spans
//! whole path segments; `*`/`?` match within one. Each glob carries a partial-order
//! specificity that makes cross-dimension overlaps ambiguous (§FS-001-config.3.2).

use std::cmp::Ordering;

/// A compiled glob pattern with precomputed specificity features.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Glob {
    pattern: String,
    segments: Vec<String>,
    spec: GlobSpec,
}

/// The specificity features of a single glob, compared as a partial order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlobSpec {
    /// Number of leading literal path segments before the first wildcard.
    prefix_depth: usize,
    /// Literal (non-wildcard) characters in the final segment — a longer, more
    /// specific extension constraint such as `*.gen.rs` outranks `*.rs`.
    ext_literal: usize,
    /// Total literal characters across the whole pattern; the final tie-break.
    literal_len: usize,
}

impl Glob {
    /// Compile a glob pattern. Compilation never fails; an empty or malformed
    /// pattern simply matches little or nothing.
    pub fn new(pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        let segments: Vec<String> = pattern.split('/').map(str::to_owned).collect();
        let spec = GlobSpec::from_segments(&segments);
        Self {
            pattern,
            segments,
            spec,
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn spec(&self) -> GlobSpec {
        self.spec
    }

    /// Whether this glob matches `path` (a `/`-separated repo-relative path).
    pub fn matches(&self, path: &str) -> bool {
        let path_segments: Vec<&str> = path.split('/').collect();
        match_segments(&self.segments, &path_segments)
    }

    /// Whether this glob and another glob can match at least one common path
    /// (§FS-003-exceptions.3), using the same overlap decision as validation.
    pub fn intersects(&self, other: &Glob) -> bool {
        super::glob_overlap::intersects(self, other)
    }
}

impl GlobSpec {
    fn from_segments(segments: &[String]) -> Self {
        let prefix_depth = segments
            .iter()
            .take_while(|segment| !is_wildcard_segment(segment))
            .count();

        let ext_literal = segments
            .last()
            .map(|segment| literal_chars(segment))
            .unwrap_or(0);

        let literal_len = segments.iter().map(|segment| literal_chars(segment)).sum();

        Self {
            prefix_depth,
            ext_literal,
            literal_len,
        }
    }

    /// Partial-order specificity comparison. `Greater`/`Less` when one glob
    /// dominates; `Equal` when they tie *or* are incomparable, which the caller
    /// resolves as ambiguity unless a rule priority breaks it (§FS-001-config.3.2).
    pub fn cmp_specificity(&self, other: &Self) -> Ordering {
        let depth = self.prefix_depth.cmp(&other.prefix_depth);
        let ext = self.ext_literal.cmp(&other.ext_literal);

        match (depth, ext) {
            // One side dominates on every axis (with at least one strict win).
            (Ordering::Greater, Ordering::Greater | Ordering::Equal)
            | (Ordering::Equal, Ordering::Greater) => Ordering::Greater,
            (Ordering::Less, Ordering::Less | Ordering::Equal)
            | (Ordering::Equal, Ordering::Less) => Ordering::Less,
            // Equal on both primary axes: fall back to total literal length.
            (Ordering::Equal, Ordering::Equal) => self.literal_len.cmp(&other.literal_len),
            // Specific in different dimensions: incomparable -> ambiguous.
            _ => Ordering::Equal,
        }
    }
}

fn is_wildcard_segment(segment: &str) -> bool {
    segment.contains('*') || segment.contains('?')
}

fn literal_chars(segment: &str) -> usize {
    segment
        .chars()
        .filter(|character| *character != '*' && *character != '?')
        .count()
}

/// Recursive segment match with `**` spanning zero or more whole segments.
fn match_segments(pattern: &[String], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((head, rest)) if head == "**" => {
            // `**` consumes any number of path segments, including none.
            (0..=path.len()).any(|skip| match_segments(rest, &path[skip..]))
        }
        Some((head, rest)) => match path.split_first() {
            Some((first, tail)) if match_one(head, first) => match_segments(rest, tail),
            _ => false,
        },
    }
}

/// Match a single non-`**` pattern segment against one path segment.
fn match_one(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    match_within(&pattern, &text)
}

fn match_within(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => {
            // `*` matches any run of characters within the segment.
            (0..=text.len()).any(|skip| match_within(rest, &text[skip..]))
        }
        Some(('?', rest)) => !text.is_empty() && match_within(rest, &text[1..]),
        Some((literal, rest)) => match text.split_first() {
            Some((first, tail)) if first == literal => match_within(rest, tail),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_double_star_and_extension() {
        let glob = Glob::new("src/**/*.rs");
        assert!(glob.matches("src/lib.rs"));
        assert!(glob.matches("src/a/b/c.rs"));
        assert!(!glob.matches("benches/x.rs"));
        assert!(!glob.matches("src/lib.md"));
    }

    #[test]
    fn catch_all_and_single_star() {
        assert!(Glob::new("**/*").matches("any/where/file.txt"));
        assert!(Glob::new("**/*.toml").matches("a/b/Cargo.toml"));
        assert!(Glob::new("*.min.*").matches("app.min.js"));
        assert!(!Glob::new("*.min.*").matches("app.js"));
    }

    #[test]
    fn deeper_prefix_dominates_when_extension_ties() {
        let shallow = Glob::new("docs/**/*.md").spec();
        let deep = Glob::new("docs/api/**/*.md").spec();
        assert_eq!(deep.cmp_specificity(&shallow), Ordering::Greater);
        assert_eq!(shallow.cmp_specificity(&deep), Ordering::Less);
    }

    #[test]
    fn extension_dominates_catch_all() {
        let any = Glob::new("**/*").spec();
        let ext = Glob::new("**/*.rs").spec();
        assert_eq!(ext.cmp_specificity(&any), Ordering::Greater);
    }

    #[test]
    fn cross_dimension_specificity_is_ambiguous() {
        // Deeper prefix vs. more specific extension: neither dominates.
        let generated = Glob::new("src/**/*.gen.rs").spec();
        let domain = Glob::new("src/domain/**/*.rs").spec();
        assert_eq!(generated.cmp_specificity(&domain), Ordering::Equal);
        assert_eq!(domain.cmp_specificity(&generated), Ordering::Equal);
    }

    #[test]
    fn detects_glob_intersections() {
        assert!(Glob::new("src/**").intersects(&Glob::new("src/**/*.rs")));
        assert!(Glob::new("src/**/*.rs").intersects(&Glob::new("src/lib.rs")));
        assert!(Glob::new("src/*.rs").intersects(&Glob::new("src/a?c.*")));
        assert!(!Glob::new("src/**/*.rs").intersects(&Glob::new("docs/**/*.md")));
        assert!(!Glob::new("src/*.rs").intersects(&Glob::new("src/*.md")));
    }
}
