use std::collections::{BTreeSet, HashMap};

use cargo_metadata::{DependencyKind, Metadata, PackageId};
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef as _;
use petgraph::{Directed, Graph, algo::is_cyclic_directed};

use crate::analyzer::{CrateInfo, CrateKind};

#[derive(Debug, Clone)]
struct Node {
    id: PackageId,
    // All the different ways this package is used
    kind: BTreeSet<CrateKind>,
}

/// Directed edge data
#[derive(Debug, Clone)]
struct Edge {
    // how the dependent is using the dependee
    kind: BTreeSet<CrateKind>,
}

struct Dag {
    graph: Graph<Node, Edge, Directed>,
    package_to_node: HashMap<PackageId, NodeIndex>,
}

impl Dag {
    fn new() -> Self {
        Self {
            graph: Graph::new(),
            package_to_node: HashMap::new(),
        }
    }

    fn get_or_create_node(&mut self, package_id: PackageId) -> NodeIndex {
        if let Some(&node_idx) = self.package_to_node.get(&package_id) {
            node_idx
        } else {
            let node = Node {
                id: package_id.clone(),
                kind: BTreeSet::new(),
            };
            let node_idx = self.graph.add_node(node);
            self.package_to_node.insert(package_id, node_idx);
            node_idx
        }
    }

    fn build(
        &mut self,
        metadata: &Metadata,
        starting_packages: &[PackageId],
    ) -> anyhow::Result<()> {
        use std::collections::VecDeque;

        let mut queue = VecDeque::new();
        let mut visited = std::collections::HashSet::new();

        // Add all starting packages to the queue
        for package_id in starting_packages {
            if visited.insert(package_id.clone()) {
                queue.push_back(package_id.clone());
            }
        }

        while let Some(package_id) = queue.pop_front() {
            let dependent_idx = self.get_or_create_node(package_id.clone());

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
                    let dependency_idx = self.get_or_create_node(dep_package.id.clone());

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
                    self.graph.add_edge(dependent_idx, dependency_idx, edge);

                    // Add dependency to queue if not already visited
                    if visited.insert(dep_package.id.clone()) {
                        queue.push_back(dep_package.id.clone());
                    }
                }
            }
        }

        Ok(())
    }
}

pub fn analyze_dependency_dag(
    metadata: &Metadata,
    sinks: &[PackageId],
) -> anyhow::Result<HashMap<PackageId, CrateInfo>> {
    let mut dag = Dag::new();

    dag.build(metadata, sinks)?;

    // Mark all the sink nodes as Normal
    for sink in sinks {
        if let Some(&node_idx) = dag.package_to_node.get(sink) {
            dag.graph[node_idx].kind.insert(CrateKind::Normal);
        }
    }

    // Flood-fill the Node kinds by visiting the graph in topological order
    use petgraph::algo::toposort;
    let topo_order = toposort(&dag.graph, None)
        .map_err(|_ignored| anyhow::anyhow!("The dependency graph has cycles"))?;

    // Process nodes in reverse topological order (from sinks backwards)
    for &node_idx in topo_order.iter().rev() {
        let node_kind = dag.graph[node_idx].kind.clone();

        // Collect edge information to avoid borrow checker issues
        let edges: Vec<(NodeIndex, Edge)> = dag
            .graph
            .edges(node_idx)
            .map(|edge| (edge.target(), edge.weight().clone()))
            .collect();
        for (dependency_idx, edge_data) in edges {
            // Calculate the new kinds for the dependency
            let new_kinds = depenency_kind_from_edge_and_dependent(
                &edge_data,
                &Node {
                    id: dag.graph[node_idx].id.clone(),
                    kind: node_kind.clone(),
                },
            );

            // Union with existing kinds
            dag.graph[dependency_idx].kind.extend(new_kinds);
        }
    }

    // Convert to CrateInfo map
    let mut result = HashMap::new();
    #[expect(clippy::iter_over_hash_type)] // Order doesn't matter for building result map
    for (package_id, &node_idx) in &dag.package_to_node {
        let node = &dag.graph[node_idx];
        let crate_info = CrateInfo {
            kind: node.kind.clone(),
        };
        result.insert(package_id.clone(), crate_info);
    }

    Ok(result)
}

/// We are looking at a dependency.
/// How should we color the dependency with `kind`?
fn depenency_kind_from_edge_and_dependent(edge: &Edge, dependent: &Node) -> BTreeSet<CrateKind> {
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
