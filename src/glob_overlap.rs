//! File-independent glob intersection after exclusions (§FS-003-exceptions.4).
//! Each segment consumes its trailing slash, including the final segment. Thus
//! `**` is a loop over whole segments with an epsilon exit, including zero
//! segments (§FS-001-config.3). Literal braces remain ordinary characters.

use std::collections::HashSet;

use crate::{Glob, Selector};

/// All overlap callers use the same language decision (§FS-003-exceptions.3).
pub(crate) fn intersects(left: &Glob, right: &Glob) -> bool {
    common_path([Machine::glob(left), Machine::glob(right)], &[])
}

/// Whether an include and the entry share a path outside the union of excludes
/// (§FS-003-exceptions.4). No filesystem inventory or precedence is involved.
pub(crate) fn reaches(entry: &Glob, selector: &Selector, excludes: &[Glob]) -> bool {
    match selector {
        Selector::Glob(includes) => includes
            .iter()
            .any(|include| common_path([Machine::glob(entry), Machine::glob(include)], excludes)),
        _ => common_path(
            [Machine::glob(entry), Machine::selector(selector)],
            excludes,
        ),
    }
}

/// Symbolic alphabet: each literal plus one representative of every other
/// character is sufficient for `*` and `?` (§FS-001-config.3).
#[derive(Clone, Copy)]
enum Symbol {
    Literal(char),
    NonSlash,
    Any,
}

impl Symbol {
    /// Wildcards distinguish separators from segment characters (§FS-001-config.3).
    fn accepts(self, character: char) -> bool {
        match self {
            Self::Literal(expected) => expected == character,
            Self::NonSlash => character != '/',
            Self::Any => true,
        }
    }
}

/// An epsilon NFA for the shipped glob language (§FS-001-config.3).
#[derive(Default)]
struct Node {
    epsilon: Vec<usize>,
    edges: Vec<(Symbol, usize)>,
}

#[derive(Default)]
/// One compiled path language, with its final node accepting (§FS-001-config.3).
struct Machine {
    nodes: Vec<Node>,
}

impl Machine {
    /// Begin at the empty prefix of a path language (§FS-001-config.3).
    fn new() -> Self {
        Self {
            nodes: vec![Node::default()],
        }
    }

    /// Append one required character (§FS-001-config.3).
    fn push(&mut self, symbol: Symbol) {
        let next = self.nodes.len();
        self.nodes[next - 1].edges.push((symbol, next));
        self.nodes.push(Node::default());
    }

    /// Append zero or more characters (§FS-001-config.3).
    fn repeat(&mut self, symbol: Symbol) {
        let current = self.nodes.len() - 1;
        self.nodes[current].edges.push((symbol, current));
    }

    /// Preserve literal Unicode characters, including braces (§FS-001-config.3).
    fn literal(&mut self, text: &str) {
        for character in text.chars() {
            self.push(Symbol::Literal(character));
        }
    }

    /// A whole-segment double star can exit without consuming a separator, or
    /// consume any segment and return to its start (§FS-001-config.3).
    fn double_star(&mut self) {
        let start = self.nodes.len() - 1;
        self.push(Symbol::NonSlash);
        self.repeat(Symbol::NonSlash);
        let inside = self.nodes.len() - 1;
        self.nodes[inside].edges.push((Symbol::Literal('/'), start));
        self.nodes[start].edges.push((Symbol::Literal('/'), start));
        self.nodes.push(Node::default());
        let end = self.nodes.len() - 1;
        self.nodes[start].epsilon.push(end);
    }

    /// Preserve segment-local stars and questions; only a segment consisting
    /// of exactly `**` spans separators (§FS-001-config.3).
    fn glob(glob: &Glob) -> Self {
        let mut machine = Self::new();
        for segment in glob.pattern().split('/') {
            if segment == "**" {
                machine.double_star();
                continue;
            }
            for character in segment.chars() {
                match character {
                    '*' => machine.repeat(Symbol::NonSlash),
                    '?' => machine.push(Symbol::NonSlash),
                    literal => machine.push(Symbol::Literal(literal)),
                }
            }
            machine.push(Symbol::Literal('/'));
        }
        machine
    }

    /// Retain library selectors as literal path predicates, without interpreting
    /// metacharacters in exact paths, prefixes or extensions (§FS-003-exceptions.4).
    fn selector(selector: &Selector) -> Self {
        let mut machine = Self::new();
        match selector {
            Selector::All => machine.repeat(Symbol::Any),
            Selector::Prefix(prefix) => {
                machine.literal(prefix);
                machine.repeat(Symbol::Any);
            }
            Selector::Exact(path) => machine.literal(path),
            Selector::Extension(extension) => {
                let extension = extension.trim_start_matches('.');
                // A final extension contains neither a dot nor a separator;
                // the filename must have a character before its final dot.
                if extension.contains(['.', '/']) {
                    machine.nodes.push(Node::default());
                } else {
                    machine.repeat(Symbol::Any);
                    machine.push(Symbol::NonSlash);
                    machine.repeat(Symbol::NonSlash);
                    machine.push(Symbol::Literal('.'));
                    machine.literal(extension);
                }
            }
            Selector::Glob(_) => unreachable!("includes are compiled individually"),
        }
        machine.push(Symbol::Literal('/'));
        machine
    }

    /// Close excluded-language subsets under epsilon transitions
    /// (§FS-003-exceptions.4). Sorted subsets give canonical visited keys.
    fn close(&self, mut states: Vec<usize>) -> Vec<usize> {
        let mut seen = vec![false; self.nodes.len()];
        let mut index = 0;
        while index < states.len() {
            let state = states[index];
            index += 1;
            if !seen[state] {
                seen[state] = true;
                states.extend(&self.nodes[state].epsilon);
            }
        }
        seen.iter()
            .enumerate()
            .filter_map(|(i, &set)| set.then_some(i))
            .collect()
    }

    /// Advance every possible match by one character (§FS-001-config.3).
    fn step(&self, states: &[usize], character: char) -> Vec<usize> {
        self.close(
            states
                .iter()
                .flat_map(|&state| {
                    self.nodes[state]
                        .edges
                        .iter()
                        .filter_map(move |&(symbol, next)| {
                            symbol.accepts(character).then_some(next)
                        })
                })
                .collect(),
        )
    }

    /// A full path reaches the final node after its last slash (§FS-001-config.3).
    fn accepts(&self, states: &[usize]) -> bool {
        states.last() == Some(&(self.nodes.len() - 1))
    }
}

/// Positive matches need one accepting run each; an exclusion must retain
/// every run to decide that none accepts (§FS-003-exceptions.4).
#[derive(Clone, Eq, Hash, PartialEq)]
struct ProductState {
    required: [usize; 2],
    excluded: Vec<Vec<usize>>,
    has_segment: bool,
}

/// Keep required NFAs nondeterministic (§GOAL-001-fast-feedback.1). Without
/// exclusions, the state count is at most twice the product of their node counts.
/// Only exclusion complements may require exponentially many subsets
/// (§FS-003-exceptions.4); the decision has no depth or time cutoff.
struct Search {
    required: [Machine; 2],
    excluded: Vec<Machine>,
    alphabet: Vec<char>,
    visited: HashSet<ProductState>,
}

impl Search {
    /// Partition characters by all literal and separator predicates
    /// (§FS-001-config.3), including those in the excluded languages.
    fn new(required: [Machine; 2], excludes: &[Glob]) -> Self {
        let excluded: Vec<_> = excludes.iter().map(Machine::glob).collect();
        let mut alphabet = vec!['/'];
        for machine in required.iter().chain(&excluded) {
            for node in &machine.nodes {
                for &(symbol, _) in &node.edges {
                    if let Symbol::Literal(character) = symbol {
                        alphabet.push(character);
                    }
                }
            }
        }
        alphabet.sort_unstable();
        alphabet.dedup();
        let other = (0..)
            .filter_map(char::from_u32)
            .find(|c| !alphabet.contains(c))
            .unwrap();
        alphabet.push(other);
        Self {
            required,
            excluded,
            alphabet,
            visited: HashSet::new(),
        }
    }

    /// Follow independent positive epsilon moves and synchronized character
    /// moves. Subtract the union of exclusions, even if it covers only the
    /// intersection (§FS-003-exceptions.4).
    fn common_path(&mut self) -> bool {
        let start = ProductState {
            required: [0, 0],
            excluded: self
                .excluded
                .iter()
                .map(|machine| machine.close(vec![0]))
                .collect(),
            has_segment: false,
        };
        let mut pending = vec![start];
        while let Some(state) = pending.pop() {
            if !self.visited.insert(state.clone()) {
                continue;
            }
            // A path split has a segment even for the empty string: reject
            // the zero-segment epsilon route through `**` (§FS-001-config.3).
            if state.has_segment
                && self
                    .required
                    .iter()
                    .zip(state.required)
                    .all(|(machine, node)| node == machine.nodes.len() - 1)
                && !self
                    .excluded
                    .iter()
                    .zip(&state.excluded)
                    .any(|(machine, states)| machine.accepts(states))
            {
                return true;
            }
            for (i, machine) in self.required.iter().enumerate() {
                for &next in &machine.nodes[state.required[i]].epsilon {
                    let mut successor = state.clone();
                    successor.required[i] = next;
                    pending.push(successor);
                }
            }
            let left = &self.required[0].nodes[state.required[0]];
            let right = &self.required[1].nodes[state.required[1]];
            for &character in &self.alphabet {
                for &(left_symbol, left_next) in &left.edges {
                    if !left_symbol.accepts(character) {
                        continue;
                    }
                    for &(right_symbol, right_next) in &right.edges {
                        if right_symbol.accepts(character) {
                            pending.push(ProductState {
                                required: [left_next, right_next],
                                excluded: self
                                    .excluded
                                    .iter()
                                    .zip(&state.excluded)
                                    .map(|(machine, states)| machine.step(states, character))
                                    .collect(),
                                has_segment: state.has_segment || character == '/',
                            });
                        }
                    }
                }
            }
        }
        false
    }
}

/// One exact overlap decision shared by entry/removal and validation callers
/// (§FS-003-exceptions.3, §FS-003-exceptions.4).
fn common_path(required: [Machine; 2], excludes: &[Glob]) -> bool {
    Search::new(required, excludes).common_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Count product states rather than timing an exponential regression
    /// (§GOAL-001-fast-feedback.1).
    #[test]
    fn disjoint_question_suffixes_stay_within_the_nfa_product() {
        for questions in [18, 22] {
            let left = Glob::new(format!("src/*a{}.rs", "?".repeat(questions)));
            let right = Glob::new("src/*.md");
            for (left, right) in [(&left, &right), (&right, &left)] {
                let required = [Machine::glob(left), Machine::glob(right)];
                let bound = 2 * required[0].nodes.len() * required[1].nodes.len();
                let mut search = Search::new(required, &[]);
                assert!(!search.common_path());
                assert!(
                    search.visited.len() <= bound,
                    "{} product states exceed the {bound}-state NFA bound",
                    search.visited.len()
                );
                assert!(!left.intersects(right));
                assert!(!reaches(left, &Selector::Glob(vec![right.clone()]), &[]));
            }
        }
    }
}
