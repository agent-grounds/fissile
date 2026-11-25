//! Bulk ancestry and reusable nearest-file frontiers (§FS-004-check-audit.1.4.1).

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use super::Record;

/// Unfiltered reachable parents, indexed newest to oldest in topological order
/// (§FS-004-check-audit.1.4.1). Unlike a path-limited log, this includes commits
/// that did not touch the file and does not rewrite parent edges.
pub(super) struct ParentGraph {
    indices: HashMap<String, usize>,
    parents: Vec<Vec<usize>>,
}

impl ParentGraph {
    /// Acquire ancestry once per command, shared by every candidate walk
    /// (§FS-004-check-audit.1.4.1). Missing parents or invalid order cannot
    /// establish a history proof; shallow roots remain checked by the walker.
    pub(super) fn read(root: &Path, start: &str) -> io::Result<Self> {
        let output = crate::revision::git(
            root,
            &["rev-list", "--parents", "--topo-order", start, "--"],
        )?;
        if !output.status.success() {
            return Err(io::Error::other("git rev-list --parents failed"));
        }
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| io::Error::other("invalid commit graph encoding"))?;
        let rows: Vec<Vec<&str>> = text
            .lines()
            .map(|line| line.split_whitespace().collect())
            .collect();
        let mut indices = HashMap::new();
        for (index, row) in rows.iter().enumerate() {
            let Some(commit) = row.first() else {
                return Err(io::Error::other("empty commit graph record"));
            };
            if indices.insert((*commit).to_owned(), index).is_some() {
                return Err(io::Error::other("duplicate commit graph record"));
            }
        }
        let mut parents = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let mut edges = Vec::new();
            for parent in &row[1..] {
                let Some(&parent) = indices.get(*parent) else {
                    return Err(io::Error::other("missing commit graph parent"));
                };
                if parent <= index {
                    return Err(io::Error::other("invalid commit graph order"));
                }
                edges.push(parent);
            }
            parents.push(edges);
        }
        Ok(Self { indices, parents })
    }

    /// Derive the nearest relevant predecessors with an oldest-to-newest pass
    /// (§FS-004-check-audit.1.4.1). Untouched commits reuse their parents'
    /// frontiers; a relevant commit replaces that frontier with itself. These
    /// are graph facts only: measuring blobs and closing reset/scope/absence
    /// branches remains the walker's job, including its unique-visit rule.
    ///
    /// On a linear history every frontier has at most one entry, so each commit
    /// and parent edge takes constant work and space. Merges can carry wider
    /// frontiers and require additional traversals to remove redundant ancestors.
    pub(super) fn predecessors(&self, records: &[Record]) -> io::Result<Vec<Vec<usize>>> {
        let mut record_at = vec![None; self.parents.len()];
        let mut record_nodes = Vec::with_capacity(records.len());
        for (index, record) in records.iter().enumerate() {
            let Some(&node) = self.indices.get(&record.commit) else {
                return Err(io::Error::other("file record missing from commit graph"));
            };
            if record_at[node].replace(index).is_some() {
                return Err(io::Error::other("duplicate file history record"));
            }
            record_nodes.push(node);
        }

        let mut nearest: Vec<Vec<usize>> = vec![Vec::new(); self.parents.len()];
        let mut predecessors = vec![Vec::new(); records.len()];
        for node in (0..self.parents.len()).rev() {
            let mut frontier: Vec<usize> = self.parents[node]
                .iter()
                .flat_map(|&parent| nearest[parent].iter().copied())
                .collect();
            if self.parents[node].len() > 1 {
                frontier.sort_unstable();
                frontier.dedup();
                self.remove_redundant(&mut frontier, &record_nodes);
            }
            if let Some(record) = record_at[node] {
                if frontier.iter().any(|&older| older <= record) {
                    return Err(io::Error::other("invalid file history order"));
                }
                predecessors[record] = frontier;
                nearest[node] = vec![record];
            } else {
                nearest[node] = frontier;
            }
        }
        Ok(predecessors)
    }

    /// Keep only maximal relevant ancestors (§FS-004-check-audit.1.4.1). A
    /// merge can reach an older record both directly and through a newer one;
    /// retaining both could walk behind a reset. Traverse the union of candidate
    /// ancestors once, bounded by the oldest candidate's topological position.
    /// Singleton frontiers do no reachability work, avoiding all-pairs work on
    /// linear histories. No external ancestry queries or recursive stack needed.
    fn remove_redundant(&self, frontier: &mut Vec<usize>, record_nodes: &[usize]) {
        if frontier.len() < 2 {
            return;
        }
        let oldest = frontier.iter().map(|&record| record_nodes[record]).max();
        let Some(oldest) = oldest else { return };
        let mut pending: Vec<usize> = frontier
            .iter()
            .flat_map(|&record| self.parents[record_nodes[record]].iter().copied())
            .collect();
        let mut reached = HashSet::new();
        while let Some(node) = pending.pop() {
            if node > oldest || !reached.insert(node) {
                continue;
            }
            pending.extend(self.parents[node].iter().copied());
        }
        frontier.retain(|&record| !reached.contains(&record_nodes[record]));
    }
}
