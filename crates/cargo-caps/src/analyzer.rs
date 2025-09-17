use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::{
    CrateName,
    build_graph_analysis::has_build_rs,
    capability::{Capability, CapabilitySet, DeducedCapabilities},
    reservoir_sample::ReservoirSampleExt as _,
};
use cargo_metadata::{Artifact, Package, TargetKind, camino::Utf8Path};
use itertools::Itertools as _;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CrateKind {
    Unknown,
    Normal,
    Build,
    Dev,
    ProcMacro,
}

impl std::fmt::Display for CrateKind {
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

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct CrateInfo {
    pub kind: BTreeSet<CrateKind>,
}

pub struct CapsAnalyzer {
    crate_caps: HashMap<CrateName, BTreeMap<TargetKind, DeducedCapabilities>>,
    ignored_caps: CapabilitySet,
    show_empty: bool,
    pub num_skipped_artifacts: usize,
}

impl CapsAnalyzer {
    pub fn new(ignored_caps: CapabilitySet, show_empty: bool) -> Self {
        Self {
            crate_caps: HashMap::new(),
            ignored_caps,
            show_empty,
            num_skipped_artifacts: 0,
        }
    }

    /// NOTE: each crate can have multiple artifacts, e.g. both a `custom-build` (build.rs)
    /// and a library.
    ///
    /// Returns `true` if we printed anything
    pub fn add_artifact(
        &mut self,
        package: &Package,
        artifact: &Artifact,
        bin_path: &Utf8Path,
        crate_info: Option<&CrateInfo>,
        verbose: bool,
    ) -> anyhow::Result<bool> {
        let crate_name = CrateName::new(package.name.to_string())?;

        let mut deduced_caps = deduce_caps_of_binary(bin_path)?;

        debug_assert_eq!(
            artifact.target.kind.len(),
            1,
            "Expected a single, kind, got {:?}",
            artifact.target.kind
        );
        let artifact_kind = &artifact.target.kind[0];

        if matches!(
            artifact_kind,
            &TargetKind::CustomBuild | &TargetKind::ProcMacro
        ) {
            // build.rs files and proc-macros are binaries with a main function and evertyhing.
            // There is very little they can't do.
            // So they will always be sus
            return Ok(false);
        }

        deduced_caps.unknown_crates.remove(&crate_name); // we know ourselves

        for (dep_crate_name, _) in std::mem::take(&mut deduced_caps.unknown_crates) {
            if let Some(crate_caps) = self.crate_caps.get(&dep_crate_name) {
                if let Some(dep_caps) = crate_caps.get(&TargetKind::Lib) {
                    deduced_caps
                        .known_crates
                        .entry(dep_crate_name)
                        .or_default()
                        .extend(dep_caps.total_capabilities());
                } else {
                    // TODO: return error?
                    println!(
                        "{crate_name} depends on '{dep_crate_name}', but we have no Lib capabilities stored for it, only {:?}",
                        crate_caps.keys()
                    );
                }
            } else {
                // We depend on a crate that produced no build artifact.
                // It means it has no symbols of itself, and all references to it
                // are really references to this library.
                // Example: dependencies: addr2line, gimli, hashbrown, proc_macro
                // println!("{crate_name} depends on '{dep_crate_name}' which we haven't compiled");
            }
        }

        {
            let crate_caps = self.crate_caps.entry(crate_name.clone()).or_default();

            for kind in &artifact.target.kind {
                let prev = crate_caps.insert(kind.clone(), deduced_caps.clone());
                debug_assert!(prev.is_none(), "Added {crate_name} {kind} twice");
            }
        }

        if has_build_rs(package) {
            // Insert this _after_ storing it to self.crate_caps
            // so that it is not contagious.
            // TODO: should probably label proc-macros as dangerous too
            deduced_caps
                .own_caps
                .entry(Capability::BuildRs)
                .or_default();
        }

        let crate_kind_suffix = {
            if artifact.target.kind.contains(&TargetKind::CustomBuild) {
                " (build.rs)".to_owned()
            } else if artifact.target.kind.contains(&TargetKind::ProcMacro) {
                " (proc-macro)".to_owned()
            } else if let Some(crate_info) = crate_info {
                if crate_info.kind.contains(&CrateKind::Normal) {
                    String::new() // Not worth mentioning
                } else {
                    format!(" ({})", crate_info.kind.iter().join(", "))
                }
            } else if artifact.target.kind.contains(&TargetKind::Lib) {
                String::new() // Not worth mentioning
            } else {
                format!(" ({})", artifact.target.kind.iter().join(", "))
            }
        };

        let info = if !deduced_caps.unknown_symbols.is_empty() {
            let symbol_names: Vec<String> = deduced_caps
                .unknown_symbols
                .iter()
                .reservoir_sample(3)
                .iter()
                .map(|s| s.format(false))
                .collect();
            let symbol_text = if deduced_caps.unknown_symbols.len() > 3 {
                format!("{}, …", symbol_names.join(", "))
            } else {
                symbol_names.join(", ")
            };

            format!(
                "{}Any because of {} unknown symbol(s): {symbol_text}",
                Capability::Any.emoji(),
                deduced_caps.unknown_symbols.len(),
            )
        } else if deduced_caps.own_caps.is_empty() {
            let all_crate_deps: CapabilitySet = deduced_caps
                .known_crates
                .values()
                .flatten()
                .copied()
                .collect();
            let crate_deps = filter_capabilities(&all_crate_deps, &self.ignored_caps);

            if crate_deps.is_empty() {
                if self.show_empty {
                    "😌 none".to_owned()
                } else {
                    return Ok(false); // TODO: respect verbose? maybe?
                }
            } else {
                let cap_names: String = crate_deps
                    .iter()
                    .map(|cap| format!("{}{cap}", cap.emoji()))
                    .join(", ");
                format!("{cap_names} because of dependencies")
            }
        } else if let Some(reasons) = deduced_caps.own_caps.get(&Capability::Any) {
            // Why do we think this crate needs the `Any` capability?
            let mut info = format!("{}Any because of", Capability::Any.emoji());
            // TODO: pick a random reasons instead of the first N
            let max_width = 60;
            for symbol in reasons.iter().reservoir_sample(5) {
                if info.len() < max_width {
                    info += &format!(" {}", symbol.format(false));
                } else {
                    info += " …";
                    break;
                }
            }
            info
        } else {
            // Filter out ignored capabilities
            let total_caps = deduced_caps.total_capabilities();
            let filtered_caps = filter_capabilities(&total_caps, &self.ignored_caps);

            // Check if we should skip this crate (no capabilities after filtering)
            if filtered_caps.is_empty() && !self.show_empty {
                return Ok(false); // TODO: respect verbose? maybe?
            }

            // Print short description using filtered capabilities
            if filtered_caps.is_empty() {
                "😌 none".to_owned()
            } else if filtered_caps.contains(&Capability::Any) {
                // If "Any" is present, show only that
                let reasons = deduced_caps
                    .known_crates
                    .iter()
                    .filter_map(|(name, caps)| {
                        caps.contains(&Capability::Any).then_some(name.clone())
                    })
                    .collect_vec();
                let dep_word = if reasons.len() == 1 {
                    "dependency"
                } else {
                    "dependencies"
                };
                let reasons = reasons.iter().join(", ");
                format!(
                    "{}Any because of {dep_word} on {reasons}",
                    Capability::Any.emoji()
                )
            } else {
                let cap_names: Vec<String> = filtered_caps
                    .iter()
                    .map(|cap| format!("{}{cap}", cap.emoji()))
                    .collect();
                cap_names.join(", ")
            }
        };

        println!("{crate_name}{crate_kind_suffix}: {info}");
        if verbose {
            println!("  source: {}", artifact.target.src_path);
            println!("  path: {}", as_relative_path(bin_path));

            let features = &artifact.features;
            if features.is_empty() {
                println!("  features: (default)");
            } else {
                println!("  features: {}", features.join(", "));
            }

            println!("  Artifact kind: {artifact_kind}");
            if let Some(crate_info) = crate_info {
                println!("  Crate kind: {}", crate_info.kind.iter().join(", "));
            }

            if artifact_kind != &TargetKind::CustomBuild && has_build_rs(package) {
                let build_rs_caps = self
                    .crate_caps
                    .get(&crate_name)
                    .and_then(|crate_caps| crate_caps.get(&TargetKind::CustomBuild));
                if let Some(build_rs_caps) = build_rs_caps {
                    println!(
                        "  {crate_name} build.rs hcapabilities: {}",
                        build_rs_caps.total_capabilities().iter().join(", ")
                    );
                } else {
                    println!("  Missing capabilities for build.rs of {crate_name}");
                }
            }

            println!();
        }

        Ok(true)
    }
}

fn as_relative_path(path: &Utf8Path) -> &Utf8Path {
    if let Ok(cwd) = std::env::current_dir()
        && let Ok(relative) = path.strip_prefix(cwd)
    {
        relative
    } else {
        path
    }
}

/// Filter capabilities by removing ignored ones and handling the Any capability.
/// If the set includes Any, remove everything but Any.
fn filter_capabilities(
    capabilities: &CapabilitySet,
    ignored_caps: &CapabilitySet,
) -> CapabilitySet {
    // If Any is present, return only Any (regardless of ignored caps)
    if capabilities.contains(&Capability::Any) {
        let mut result = CapabilitySet::new();
        result.insert(Capability::Any);
        return result;
    }

    // Otherwise, filter out ignored capabilities
    capabilities
        .iter()
        .filter(|cap| !ignored_caps.contains(cap))
        .copied()
        .collect()
}

fn deduce_caps_of_binary(path: &Utf8Path) -> anyhow::Result<DeducedCapabilities> {
    let symbols = crate::extract_symbols(path)?;
    let filtered_symbols = crate::filter_symbols(symbols, false, false);
    DeducedCapabilities::from_symbols(filtered_symbols)
}
