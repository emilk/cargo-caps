use std::collections::{BTreeSet, HashMap, VecDeque};

use cargo_metadata::{DependencyKind, Metadata, Package, PackageId, TargetKind};
use petgraph::{Directed, graph::NodeIndex, visit::EdgeRef as _};

/// How is the main target depending on a crate?
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct DepKindSet {
    pub kind: BTreeSet<DepKind>,
}

/// How is the main target depending on a crate?
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DepKind {
    Unknown,
    Normal,
    Build,
    Dev,
    ProcMacro,
}

impl std::fmt::Display for DepKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown => write!(f, "⚠️ unknown dependency type"),
            Self::Normal => write!(f, "normal dependency"),
            Self::Build => write!(f, "build-dependency"),
            Self::Dev => write!(f, "dev-dependency"),
            Self::ProcMacro => write!(f, "proc-macro"),
        }
    }
}

pub fn has_build_rs(package: &Package) -> bool {
    package
        .targets
        .iter()
        .any(|target| target.is_custom_build())
}

/// A package in the dependency graph.
#[derive(Debug, Clone)]
struct Node {
    id: PackageId,

    // All the different ways this package is used
    kind: BTreeSet<DepKind>,
}

/// "Depends on".
#[derive(Debug, Clone)]
struct Edge {
    // how the dependent is using the dependee
    kind: BTreeSet<DepKind>,
}

impl From<DepKind> for Edge {
    fn from(kind: DepKind) -> Self {
        Self {
            kind: std::iter::once(kind).collect(),
        }
    }
}

/// A graph of the dependencies between packages,
/// with edges pointing from dependent to dependee,
/// so an edge means "depends on".
#[derive(Default)]
struct DepGraph {
    graph: petgraph::Graph<Node, Edge, Directed>,
    package_to_node: HashMap<PackageId, NodeIndex>,
}

impl DepGraph {
    pub fn insert_node(&mut self, package_id: &PackageId, kind: DepKind) {
        let node = Node {
            id: package_id.clone(),
            kind: std::iter::once(kind).collect(),
        };
        let node_idx = self.graph.add_node(node);
        let prev = self.package_to_node.insert(package_id.clone(), node_idx);
        assert!(prev.is_none(), "Node already existed: {package_id}");
    }

    pub fn node_of(&mut self, package_id: PackageId) -> NodeIndex {
        match self.package_to_node.entry(package_id.clone()) {
            std::collections::hash_map::Entry::Occupied(occupied_entry) => *occupied_entry.get(),
            std::collections::hash_map::Entry::Vacant(vacant_entry) => {
                let node = Node {
                    id: package_id,
                    kind: BTreeSet::new(),
                };
                let node_idx = self.graph.add_node(node);
                vacant_entry.insert(node_idx);
                node_idx
            }
        }
    }

    pub fn add_edge(&mut self, dependent_: PackageId, dependency: PackageId, edge: Edge) {
        let dependent = self.node_of(dependent_);
        let dependency = self.node_of(dependency);
        self.graph.add_edge(dependent, dependency, edge);
    }

    pub fn from_metadata(
        metadata: &Metadata,
        starting_packages: &[PackageId],
    ) -> anyhow::Result<Self> {
        use std::collections::VecDeque;

        let mut graph = Self::default();

        let mut queue = VecDeque::new();
        let mut visited = std::collections::HashSet::new();

        // Add all starting packages to the queue
        for package_id in starting_packages {
            if visited.insert(package_id.clone()) {
                queue.push_back(package_id.clone());
            }

            graph.insert_node(package_id, DepKind::Normal);
        }

        while let Some(package_id) = queue.pop_front() {
            // Find the package in metadata
            let package = metadata
                .packages
                .iter()
                .find(|p| p.id == package_id)
                .ok_or_else(|| anyhow::anyhow!("Package not found: {package_id:?}"))?;

            for dep in &package.dependencies {
                // Find the dependency package
                if let Some(dep_package) = metadata
                    .packages
                    .iter()
                    .find(|p| p.name.as_str() == dep.name.as_str())
                {
                    // Create edge with dependency kind
                    let mut edge_kind = BTreeSet::new();

                    if dep_package
                        .targets
                        .iter()
                        .all(|t| t.kind.iter().all(|k| k == &TargetKind::ProcMacro))
                    {
                        edge_kind.insert(DepKind::ProcMacro);
                    } else {
                        edge_kind.insert(match dep.kind {
                            DependencyKind::Normal => DepKind::Normal,
                            DependencyKind::Build => DepKind::Build,
                            DependencyKind::Development => DepKind::Dev,
                            DependencyKind::Unknown => DepKind::Unknown,
                        });
                    }

                    if edge_kind.is_empty() {
                        println!("WARNING: dependency edge has no kind");
                    }

                    let edge = Edge { kind: edge_kind };

                    // Add edge from dependent to dependency
                    graph.add_edge(package_id.clone(), dep_package.id.clone(), edge);

                    // Add dependency to queue if not already visited
                    if visited.insert(dep_package.id.clone()) {
                        queue.push_back(dep_package.id.clone());
                    }
                }
            }
        }

        Ok(graph)
    }

    fn analyze(mut self) -> HashMap<PackageId, DepKindSet> {
        self.compute_dependency_kinds();

        self.package_to_node
            .iter()
            .map(|(package_id, &node_idx)| {
                let node = &self.graph[node_idx];
                let set = DepKindSet {
                    kind: node.kind.clone(),
                };
                (package_id.clone(), set)
            })
            .collect()
    }

    fn compute_dependency_kinds(&mut self) {
        let mut queue = VecDeque::new();

        // Start with all nodes that have non-empty 'kind' field
        #[expect(clippy::iter_over_hash_type)]
        for &node_idx in self.package_to_node.values() {
            if !self.graph[node_idx].kind.is_empty() {
                queue.push_back(node_idx);
            }
        }

        // Process nodes in the queue
        while let Some(node_idx) = queue.pop_front() {
            let node_kind = self.graph[node_idx].kind.clone();

            // Collect edge information to avoid borrow checker issues
            let node_edges: Vec<(NodeIndex, Edge)> = self
                .graph
                .edges(node_idx)
                .map(|edge| (edge.target(), edge.weight().clone()))
                .collect();

            for (dependency_idx, edge_data) in node_edges {
                // Calculate the new kinds for the dependency
                let new_kinds = dependency_kind_from_edge_and_dependent(
                    &edge_data,
                    &Node {
                        id: self.graph[node_idx].id.clone(),
                        kind: node_kind.clone(),
                    },
                );

                // Check if dependency already has all the new kinds
                let current_kinds = &self.graph[dependency_idx].kind;
                let missing_kinds: BTreeSet<_> =
                    new_kinds.difference(current_kinds).copied().collect();

                if !missing_kinds.is_empty() {
                    // Extend with missing kinds and add to queue
                    self.graph[dependency_idx].kind.extend(missing_kinds);
                    queue.push_back(dependency_idx);
                }
            }
        }
    }
}

pub fn analyze_dependency_graph(
    metadata: &Metadata,
    sinks: &[PackageId],
) -> anyhow::Result<HashMap<PackageId, DepKindSet>> {
    Ok(DepGraph::from_metadata(metadata, sinks)?.analyze())
}

/// We are looking at a dependency.
/// How should we color the dependency with `kind`?
fn dependency_kind_from_edge_and_dependent(edge: &Edge, dependent: &Node) -> BTreeSet<DepKind> {
    let mut final_set = BTreeSet::default();

    for &dep_kind in &edge.kind {
        for &crate_kind in &dependent.kind {
            #[expect(clippy::match_same_arms)]
            let new_kind = match (dep_kind, crate_kind) {
                // All dependencies ON proc-macros are marked proc-macros:
                (_, DepKind::ProcMacro) => DepKind::ProcMacro,

                // A normal dependency inherits dependent's kind:
                (DepKind::Normal, crate_kind) => crate_kind,

                // All dependencies of a build-dependency are marked build-dependencies:
                (DepKind::Build, _) => DepKind::Build,

                // All dependencies of a dev-dependency are marked dev-dependencies:
                (DepKind::Dev, _) => DepKind::Dev,

                // All dependencies of a proc-macros are marked proc-macros:
                (DepKind::ProcMacro, _) => DepKind::ProcMacro,

                // Unknown is viral:
                (_, DepKind::Unknown) | (DepKind::Unknown, _) => DepKind::Unknown,
            };
            final_set.insert(new_kind);
        }
    }

    final_set
}

#[cfg(test)]
mod tests {
    #![allow(clippy::single_char_pattern)]

    use cargo_metadata::PackageId;

    use super::*;

    fn pid(s: &str) -> PackageId {
        PackageId { repr: s.to_owned() }
    }

    fn set(crate_kind: DepKind) -> DepKindSet {
        DepKindSet {
            kind: std::iter::once(crate_kind).collect(),
        }
    }

    #[test]
    fn test_graph() {
        let mut graph = DepGraph::default();
        graph.insert_node(&pid("binary"), DepKind::Normal);
        graph.add_edge(pid("binary"), pid("build_dep"), Edge::from(DepKind::Build));
        graph.add_edge(pid("build_dep"), pid("3rd"), Edge::from(DepKind::Normal));
        let result = graph.analyze();

        assert_eq!(&result[&pid("binary")], &set(DepKind::Normal));
        assert_eq!(&result[&pid("build_dep")], &set(DepKind::Build));
        assert_eq!(&result[&pid("3rd")], &set(DepKind::Build));
    }

    #[test]
    #[ignore = "TODO: fix graph analysis"]
    fn test_proc_macro() {
        let mut graph = DepGraph::default();
        graph.insert_node(&pid("binary"), DepKind::Normal);
        graph.insert_node(&pid("clap_derive"), DepKind::ProcMacro);
        // graph.insert_node(&pid("proc-macro2"), CrateKind::Normal);
        graph.add_edge(
            pid("binary"),
            pid("clap_derive"),
            Edge::from(DepKind::Normal),
        );
        graph.add_edge(
            pid("clap_derive"),
            pid("proc-macro2"),
            Edge::from(DepKind::Normal),
        );

        let result = graph.analyze();
        assert_eq!(&result[&pid("binary")], &set(DepKind::Normal));
        assert_eq!(&result[&pid("clap_derive")], &set(DepKind::ProcMacro));
        assert_eq!(&result[&pid("proc-macro2")], &set(DepKind::ProcMacro));
    }

    #[test]
    fn test_proc_macro2() {
        let mut graph = DepGraph::default();
        graph.insert_node(&pid("binary"), DepKind::Normal);
        // graph.insert_node(&pid("clap_derive"), DepKind::Normal);
        // graph.insert_node(&pid("proc-macro2"), DepKind::Normal);
        graph.add_edge(
            pid("binary"),
            pid("clap_derive"),
            Edge::from(DepKind::ProcMacro),
        );
        graph.add_edge(
            pid("clap_derive"),
            pid("proc-macro2"),
            Edge::from(DepKind::Normal),
        );

        let result = graph.analyze();

        assert_eq!(&result[&pid("binary")], &set(DepKind::Normal));
        assert_eq!(&result[&pid("clap_derive")], &set(DepKind::ProcMacro));
        assert_eq!(&result[&pid("proc-macro2")], &set(DepKind::ProcMacro));
    }
}
