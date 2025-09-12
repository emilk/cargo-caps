use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use capabara::capability::DeducedCapablities;

pub struct CapsAnalyzer {
    lib_caps: HashMap<String, DeducedCapablities>,
}

impl CapsAnalyzer {
    pub fn new() -> Self {
        Self {
            lib_caps: HashMap::new(),
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

        deduced_caps.unknown_crates.remove(crate_name);

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

        // Print short description
        let total_caps = deduced_caps.total_capabilities();
        let cap_list = if total_caps.is_empty() {
            "😌 none".to_owned()
        } else if total_caps.contains(&capabara::capability::Capability::Any) {
            // If "Any" is present, show only that
            format!("{} Any", capabara::capability::Capability::Any.emoji())
        } else {
            let cap_names: Vec<String> = total_caps
                .iter()
                .map(|c| format!("{} {c:?}", c.emoji()))
                .collect();
            cap_names.join(", ")
        };

        let mut warnings = Vec::new();
        if !deduced_caps.unknown_crates.is_empty() {
            warnings.push(format!(
                "unknown crates {:?}",
                deduced_caps.unknown_crates.keys()
            ));
        }
        if !deduced_caps.unknown_symbols.is_empty() {
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
            warnings.push(format!("unknown symbols: {symbol_text}"));
        }

        let warning_text = if warnings.is_empty() {
            String::new()
        } else {
            format!(" ⚠️ {}", warnings.join(", "))
        };

        println!("{crate_name}: {cap_list}{warning_text}");
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

        self.lib_caps.insert(crate_name.to_owned(), deduced_caps);
    }
}

impl Default for CapsAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

fn deduce_caps_of_binary(path: &Path) -> Option<DeducedCapablities> {
    let symbols = capabara::extract_symbols(path).ok()?;
    let filtered_symbols = capabara::filter_symbols(symbols, false, false);
    Some(DeducedCapablities::from_symbols(filtered_symbols))
}