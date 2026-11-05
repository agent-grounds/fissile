//! File-independent glob intersection after exclusions (§FS-003-exceptions.4).
//! Each segment consumes its trailing slash, including the final segment. Thus
//! `**` is a loop over whole segments with an epsilon exit, including zero
//! segments (§FS-001-config.3). Literal braces remain ordinary characters.

use std::collections::HashSet;

use crate::{Glob, Selector};

/// All overlap callers use the same language decision (§FS-003-exceptions.3).
pub(crate) fn intersects(left: &Glob, right: &Glob) -> bool {
    common_path(vec![Machine::glob(left), Machine::glob(right)], &[])
}

/// Whether an include and the entry share a path outside the union of excludes
/// (§FS-003-exceptions.4). No filesystem inventory or precedence is involved.
pub(crate) fn reaches(entry: &Glob, selector: &Selector, excludes: &[Glob]) -> bool {
    match selector {
        Selector::Glob(includes) => includes.iter().any(|include| {
            common_path(vec![Machine::glob(entry), Machine::glob(include)], excludes)
        }),
        _ => common_path(
            vec![Machine::glob(entry), Machine::selector(selector)],
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

    /// Determinize lazily: close only subsets reached by the product search
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

/// Search the finite product of two required languages and all exclusions.
/// Acceptance requires both includes and no exclude, which subtracts their
/// union even when it covers only the intersection (§FS-003-exceptions.4).
/// Worst-case subset growth is exponential; no depth cutoff approximates it.
fn common_path(mut machines: Vec<Machine>, excludes: &[Glob]) -> bool {
    machines.extend(excludes.iter().map(Machine::glob));
    let mut alphabet = vec!['/'];
    for machine in &machines {
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

    let start: Vec<_> = machines
        .iter()
        .map(|machine| machine.close(vec![0]))
        .collect();
    let mut pending = vec![(start, false)];
    let mut visited = HashSet::new();
    while let Some((states, has_segment)) = pending.pop() {
        if !visited.insert((states.clone(), has_segment)) {
            continue;
        }
        let accepts = |i: usize| machines[i].accepts(&states[i]);
        // A path split always has at least one segment, even for the empty
        // string. Do not accept the zero-segment epsilon route of `**` alone.
        if has_segment && accepts(0) && accepts(1) && !(2..machines.len()).any(accepts) {
            return true;
        }
        for &character in &alphabet {
            let next: Vec<_> = machines
                .iter()
                .zip(&states)
                .map(|(machine, state)| machine.step(state, character))
                .collect();
            if !next[0].is_empty() && !next[1].is_empty() {
                pending.push((next, has_segment || character == '/'));
            }
        }
    }
    false
}
