use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
};

use crate::{
    CrateName,
    capability::{Capability, CapabilitySet, DeducedCapabilities},
};
use cargo_metadata::{
    Artifact,
    camino::{Utf8Path, Utf8PathBuf},
};
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
    crate_caps: HashMap<CrateName, DeducedCapabilities>,
    ignored_caps: CapabilitySet,
    show_empty: bool,
    pub num_skipped: usize,
}

impl CapsAnalyzer {
    pub fn new(ignored_caps: CapabilitySet, show_empty: bool) -> Self {
        Self {
            crate_caps: HashMap::new(),
            ignored_caps,
            show_empty,
            num_skipped: 0,
        }
    }

    pub fn add_crate(
        &mut self,
        artifact: &Artifact,
        bin_path: &Utf8PathBuf,
    ) -> anyhow::Result<DeducedCapabilities> {
        let target = &artifact.target;
        let crate_name = CrateName::new(&target.name)?;
        let bin_path = PathBuf::from(bin_path.as_str()); // TODO: use camino everywhere

        let mut deduced_caps = deduce_caps_of_binary(&bin_path)?;

        deduced_caps.unknown_crates.remove(&crate_name); // we know ourselves

        for (dep_crate_name, _) in std::mem::take(&mut deduced_caps.unknown_crates) {
            if let Some(dep_caps) = self.crate_caps.get(&dep_crate_name) {
                deduced_caps
                    .known_crates
                    .entry(dep_crate_name)
                    .or_default()
                    .extend(dep_caps.total_capabilities());
            } else {
                // We depend on a crate that produced no build artifact.
                // It means it has no symbols of itself, and all references to it
                // are really references to this library.
            }
        }

        self.crate_caps
            .insert(crate_name.clone(), deduced_caps.clone());

        Ok(deduced_caps)
    }

    /// Rerturns true if we did print.
    pub fn print_crate_info(
        &self,
        artifact: &Artifact,
        crate_info: Option<&CrateInfo>,
        bin_path: &Utf8PathBuf,
        verbose: bool,
    ) -> anyhow::Result<bool> {
        let target = &artifact.target;
        let crate_name = CrateName::new(&target.name)?;
        let deduced_caps = &self.crate_caps[&crate_name];

        let crate_kind_suffix = {
            if let Some(crate_info) = crate_info {
                if crate_info.kind.contains(&CrateKind::Normal) {
                    String::new() // Not worth mentioning
                } else {
                    format!(" ({})", crate_info.kind.iter().join(", "))
                }
            } else {
                String::new()
            }
        };

        let info = if !deduced_caps.unknown_symbols.is_empty() {
            let symbol_names: Vec<String> = deduced_caps
                .unknown_symbols
                .iter()
                .take(3)
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
                    .map(|c| format!("{}{c:?}", c.emoji()))
                    .join(", ");
                format!("{cap_names} because of dependencies")
            }
        } else if let Some(reasons) = deduced_caps.own_caps.get(&Capability::Any) {
            // Why do we think this crate needs the `Any` capability?
            let mut info = format!("{}Any because of", Capability::Any.emoji());
            // TODO: pick a random reasons instead of the first N
            let max_width = 60;
            for symbol in reasons {
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
                format!("{}Any", Capability::Any.emoji())
            } else {
                let cap_names: Vec<String> = filtered_caps
                    .iter()
                    .map(|c| format!("{}{c:?}", c.emoji()))
                    .collect();
                cap_names.join(", ")
            }
        };

        println!("{crate_name}{crate_kind_suffix}: {info}");
        if verbose {
            println!("  path: {}", as_relative_path(bin_path));

            let features = &artifact.features;
            if features.is_empty() {
                println!("  features: (default)");
            } else {
                println!("  features: {}", features.join(", "));
            }

            if let Some(crate_info) = crate_info {
                println!("Kind: {}", crate_info.kind.iter().join(", "));
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

fn deduce_caps_of_binary(path: &Path) -> anyhow::Result<DeducedCapabilities> {
    let symbols = crate::extract_symbols(path)?;
    let filtered_symbols = crate::filter_symbols(symbols, false, false);
    DeducedCapabilities::from_symbols(filtered_symbols)
}
