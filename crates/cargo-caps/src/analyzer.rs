use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use capabara::capability::{Capability, CapabilitySet, DeducedCapablities};
use itertools::Itertools as _;

pub struct CapsAnalyzer {
    lib_caps: HashMap<String, DeducedCapablities>,
    ignored_caps: CapabilitySet,
    show_empty: bool,
}

impl CapsAnalyzer {
    pub fn new(ignored_caps_str: &str, show_empty: bool) -> Self {
        let ignored_caps = parse_ignored_caps(ignored_caps_str);
        Self {
            lib_caps: HashMap::new(),
            ignored_caps,
            show_empty,
        }
    }

    pub fn add_lib_or_bin(
        &mut self,
        crate_name: &str,
        bin_path: &cargo_metadata::camino::Utf8PathBuf,
        verbose: bool,
        features: Option<&[String]>,
    ) {
        let path = PathBuf::from(bin_path.as_str());

        // Analyze capabilities for this rlib
        let Some(mut deduced_caps) = deduce_caps_of_binary(&path) else {
            eprintln!("ERROR: failed to decude capabilities of {bin_path:?}"); // TODO: report error
            return;
        };

        deduced_caps.unknown_crates.remove(crate_name); // we know ourselves

        for (dep_crate_name, _) in std::mem::take(&mut deduced_caps.unknown_crates) {
            if let Some(dep_caps) = self.lib_caps.get(&dep_crate_name) {
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

        // Remember capabilities:
        self.lib_caps
            .insert(crate_name.to_owned(), deduced_caps.clone());

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
                "{}Any because of unknown symbols: {symbol_text}",
                capabara::capability::Capability::Any.emoji()
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
                    return;
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
            let mut info = format!(
                "{}Any because of",
                capabara::capability::Capability::Any.emoji()
            );
            // TODO: pick a few reasons at random instead of the first N
            let max_width = 80;
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
                // Store the capabilities but don't display
                self.lib_caps.insert(crate_name.to_owned(), deduced_caps);
                return;
            }

            // Print short description using filtered capabilities
            if filtered_caps.is_empty() {
                "😌 none".to_owned()
            } else if filtered_caps.contains(&capabara::capability::Capability::Any) {
                // If "Any" is present, show only that
                format!("{}Any", capabara::capability::Capability::Any.emoji())
            } else {
                let cap_names: Vec<String> = filtered_caps
                    .iter()
                    .map(|c| format!("{}{c:?}", c.emoji()))
                    .collect();
                cap_names.join(", ")
            }
        };

        println!("{crate_name}: {info}");
        if verbose {
            println!("  Path: {}", bin_path.as_str());
            if let Some(features) = features {
                if features.is_empty() {
                    println!("  Features: (default)");
                } else {
                    println!("  Features: {}", features.join(", "));
                }
            }
            println!();
        }
    }
}

impl Default for CapsAnalyzer {
    fn default() -> Self {
        Self::new("alloc,panic", false) // Default ignored caps and don't show empty
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

/// Parse a comma-separated string of capability names (lowercase) into a set
fn parse_ignored_caps(caps_str: &str) -> CapabilitySet {
    caps_str
        .split(',')
        .filter_map(|s| {
            let s = s.trim().to_lowercase();
            match s.as_str() {
                "panic" => Some(Capability::Panic),
                "alloc" => Some(Capability::Alloc),
                "time" => Some(Capability::Time),
                "sysinfo" => Some(Capability::Sysinfo),
                "stdio" => Some(Capability::Stdio),
                "thread" => Some(Capability::Thread),
                "net" => Some(Capability::Net),
                "fopen" => Some(Capability::Fopen),
                "any" => Some(Capability::Any),
                _ => {
                    if !s.is_empty() {
                        eprintln!("Warning: unknown capability '{s}' in ignored-caps");
                    }
                    None
                }
            }
        })
        .collect()
}

fn deduce_caps_of_binary(path: &Path) -> Option<DeducedCapablities> {
    let symbols = capabara::extract_symbols(path).ok()?;
    let filtered_symbols = capabara::filter_symbols(symbols, false, false);
    Some(DeducedCapablities::from_symbols(filtered_symbols))
}
