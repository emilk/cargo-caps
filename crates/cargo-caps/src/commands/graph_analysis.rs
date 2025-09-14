use std::collections::{BTreeSet, HashMap};

use cargo_metadata::{DependencyKind, Metadata, PackageId};
use petgraph::{Directed, graph::NodeIndex, visit::EdgeRef as _};

use crate::analyzer::{CrateInfo, CrateKind};

/// A package in the dependency graph.
#[derive(Debug, Clone)]
struct Node {
    id: PackageId,

    // All the different ways this package is used
    kind: BTreeSet<CrateKind>,
}

/// "Depends on".
#[derive(Debug, Clone)]
struct Edge {
    // how the dependent is using the dependee
    kind: BTreeSet<CrateKind>,
}

impl From<CrateKind> for Edge {
    fn from(kind: CrateKind) -> Self {
        Self {
            kind: std::iter::once(kind).collect(),
        }
    }
}

/// A graph of the dependencies between packages,
/// with edges pointing from dependent to dependee,
/// so an edge means "depends on".
#[derive(Default)]
struct Dag {
    graph: petgraph::Graph<Node, Edge, Directed>,
    package_to_node: HashMap<PackageId, NodeIndex>,
}

impl Dag {
    pub fn insert_node(&mut self, package_id: &PackageId, kind: CrateKind) {
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

        let mut dag = Self::default();

        let mut queue = VecDeque::new();
        let mut visited = std::collections::HashSet::new();

        // Add all starting packages to the queue
        for package_id in starting_packages {
            if visited.insert(package_id.clone()) {
                queue.push_back(package_id.clone());
            }

            dag.insert_node(package_id, CrateKind::Normal);
        }

        while let Some(package_id) = queue.pop_front() {
            // Find the package in metadata
            let package = metadata
                .packages
                .iter()
                .find(|p| p.id == package_id)
                .ok_or_else(|| anyhow::anyhow!("Package not found: {:?}", package_id))?;

            for dep in &package.dependencies {
                // Skip dev dependencies to avoid cycles (dev deps often create circular references)
                if dep.kind == DependencyKind::Development {
                    continue;
                }

                // Find the dependency package
                if let Some(dep_package) = metadata
                    .packages
                    .iter()
                    .find(|p| p.name.as_str() == dep.name.as_str())
                {
                    // Create edge with dependency kind
                    let mut edge_kind = BTreeSet::new();
                    edge_kind.insert(match dep.kind {
                        DependencyKind::Normal => CrateKind::Normal,
                        DependencyKind::Build => CrateKind::Build,
                        DependencyKind::Development => CrateKind::Dev,
                        DependencyKind::Unknown => CrateKind::Unknown,
                    });

                    if edge_kind.is_empty() {
                        eprintln!("WARNING: dependency edge has no kind");
                    }

                    let edge = Edge { kind: edge_kind };

                    // Add edge from dependent to dependency
                    dag.add_edge(package_id.clone(), dep_package.id.clone(), edge);

                    // Add dependency to queue if not already visited
                    if visited.insert(dep_package.id.clone()) {
                        queue.push_back(dep_package.id.clone());
                    }
                }
            }
        }

        Ok(dag)
    }

    fn anayze(mut self) -> anyhow::Result<HashMap<PackageId, CrateInfo>> {
        self.compute_dependency_kinds()?;

        Ok(self
            .package_to_node
            .iter()
            .map(|(package_id, &node_idx)| {
                let node = &self.graph[node_idx];
                let crate_info = CrateInfo {
                    kind: node.kind.clone(),
                };
                (package_id.clone(), crate_info)
            })
            .collect())
    }

    fn compute_dependency_kinds(&mut self) -> Result<(), anyhow::Error> {
        // Flood-fill the Node kinds by visiting the graph in topological order
        use petgraph::algo::toposort;
        let topo_order = toposort(&self.graph, None)
            .map_err(|_ignored| anyhow::anyhow!("The dependency graph has cycles"))?;

        // Process nodes in topological order
        for &node_idx in &topo_order {
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

                // Union with existing kinds
                self.graph[dependency_idx].kind.extend(new_kinds);
            }
        }

        Ok(())
    }
}

pub fn analyze_dependency_dag(
    metadata: &Metadata,
    sinks: &[PackageId],
) -> anyhow::Result<HashMap<PackageId, CrateInfo>> {
    Dag::from_metadata(metadata, sinks)?.anayze()
}

/// We are looking at a dependency.
/// How should we color the dependency with `kind`?
fn dependency_kind_from_edge_and_dependent(edge: &Edge, dependent: &Node) -> BTreeSet<CrateKind> {
    let mut final_set = BTreeSet::default();

    for &dep_kind in &edge.kind {
        for &crate_kind in &dependent.kind {
            let new_kind = match (dep_kind, crate_kind) {
                // Build and Dev dependencies always propagate their kind
                (CrateKind::Build, _) => CrateKind::Build,
                (CrateKind::Dev, _) => CrateKind::Dev,

                // Normal dependency relationships
                (CrateKind::Normal, CrateKind::Normal) => CrateKind::Normal,
                (CrateKind::Normal, crate_kind) => crate_kind, // normal dependency of build/dev/proc-macro crate inherits dependent's kind

                // If dependent is Unknown, dependency keeps its kind
                (dep_kind, CrateKind::Unknown) => dep_kind,

                // Unknown and ProcMacro inherit behavior
                (CrateKind::Unknown | CrateKind::ProcMacro, crate_kind) => crate_kind, // inherit dependent's kind
            };
            final_set.insert(new_kind);
        }
    }

    final_set
}

#[cfg(test)]
mod tests {
    use crate::analyzer::CrateKind;
    use cargo_metadata::PackageId;

    use super::*;

    fn pid(s: &str) -> PackageId {
        PackageId { repr: s.to_owned() }
    }

    fn crate_info(crate_kind: CrateKind) -> CrateInfo {
        CrateInfo {
            kind: std::iter::once(crate_kind).collect(),
        }
    }

    #[test]
    fn test_dag() {
        let mut dag = Dag::default();
        dag.insert_node(&pid("binary"), CrateKind::Normal);
        dag.add_edge(
            pid("binary"),
            pid("build_dep"),
            Edge::from(CrateKind::Build),
        );
        dag.add_edge(pid("build_dep"), pid("3rd"), Edge::from(CrateKind::Normal));
        let result = dag.anayze().unwrap();

        assert_eq!(&result[&pid("binary")], &crate_info(CrateKind::Normal));
        assert_eq!(&result[&pid("build_dep")], &crate_info(CrateKind::Build));
        assert_eq!(&result[&pid("3rd")], &crate_info(CrateKind::Build));
    }
}
