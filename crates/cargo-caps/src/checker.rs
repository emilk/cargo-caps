use std::collections::{BTreeMap, HashMap};

use crate::{
    CrateName,
    build_graph_analysis::{DepKind, DepKindSet, has_build_rs},
    cap_rule::SymbolRules,
    capability::{Capability, CapabilitySet, DeducedCapabilities},
    config::WorkspaceConfig,
    reservoir_sample::ReservoirSampleExt as _,
};
use cargo_metadata::{Artifact, Metadata, Package, PackageId, TargetKind, camino::Utf8Path};
use itertools::Itertools as _;

/// What [`Checker`] computers
#[derive(Default)]
pub struct CheckerOutput {
    pub crate_caps: HashMap<CrateName, BTreeMap<TargetKind, DeducedCapabilities>>,
    pub num_artifacts_passed: usize,
}

pub struct Checker {
    /// Rules for matching symbols to capabilities
    pub rules: SymbolRules,
    pub config: WorkspaceConfig,
    pub metadata: Metadata,
    pub show_empty: bool,
}

impl Checker {
    pub fn analyze_artifact(
        &self,
        output: &mut CheckerOutput,
        crate_infos: Option<&HashMap<PackageId, DepKindSet>>,
        verbose: bool,
        artifact: &cargo_metadata::Artifact,
    ) -> Result<(), anyhow::Error> {
        let package = self
            .metadata
            .packages
            .iter()
            .find(|p| p.id == artifact.package_id)
            .unwrap(); // TODO

        let set = if let Some(sets) = crate_infos {
            if let Some(set) = sets.get(&artifact.package_id) {
                if !set.kind.contains(&DepKind::Normal) {
                    return Ok(()); // ignore build dependencies, proc-macros etc - they cannot affect users machines
                }

                Some(set)
            } else {
                // Not sure why we sometimes end up here.
                // Examples: bitflags block2 objc2 objc2_app_kit memoffset rustix
                // println!("ERROR: unknown crate {}", artifact.target.name);
                return Ok(());
                // None
            }
        } else {
            None
        };

        for file_path in &artifact.filenames {
            if file_path.as_str().ends_with(".rmeta") {
                // .rmeta files has all the symbols and function signatures,
                // without any of the compiled code.
                // It what makes `cargo check` faster than `cargo build`.
                // But we cannot parse these files, so we just ignore them
            } else {
                let did_print =
                    self.add_artifact(output, package, artifact, file_path, set, verbose)?;
                if !did_print {
                    output.num_artifacts_passed += 1;
                }
            }
        }

        Ok(())
    }

    /// NOTE: each crate can have multiple artifacts, e.g. both a `custom-build` (build.rs)
    /// and a library.
    ///
    /// Returns `true` if we printed anything
    pub fn add_artifact(
        &self,
        output: &mut CheckerOutput,
        package: &Package,
        artifact: &Artifact,
        bin_path: &Utf8Path,
        crate_info: Option<&DepKindSet>,
        verbose: bool,
    ) -> anyhow::Result<bool> {
        let crate_name = CrateName::new(package.name.to_string())?;

        let allowed_caps = self.config.crate_caps(&crate_name);

        let mut deduced_caps = deduce_caps_of_binary(&self.rules, bin_path)?;

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

        if true {
            // We don't care about dependencies - we should have already have covered those.`
            // TODO: veirfy that each unknown_crate is found in the cargo_metadata dependency list,
            // or the symbol deducer might have a bug
            deduced_caps.unknown_crates.clear();
        } else {
            deduced_caps.unknown_crates.remove(&crate_name); // we know ourselves

            for (dep_crate_name, _) in std::mem::take(&mut deduced_caps.unknown_crates) {
                if let Some(crate_caps) = output.crate_caps.get(&dep_crate_name) {
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
        }

        {
            let crate_caps = output.crate_caps.entry(crate_name.clone()).or_default();

            for kind in &artifact.target.kind {
                // Append to existing, if any.
                // Why? Because we don't know on which version the symbol is referring to
                // … or do we???

                crate_caps.insert(kind.clone(), deduced_caps.clone());
                //     crate_caps
                //         .entry(kind.clone())
                //         .or_default()
                //         .union_with(deduced_caps.clone());
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
            } else if artifact.target.kind.contains(&TargetKind::Bin) {
                " (bin)".to_owned()
            } else if let Some(crate_info) = crate_info {
                if crate_info.kind.contains(&DepKind::Normal) {
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
            let crate_deps = filter_capabilities(&all_crate_deps, &allowed_caps);

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
            let filtered_caps = filter_capabilities(&total_caps, &allowed_caps);

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
                let build_rs_caps = output
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

/// Filter capabilities by removing allowed ones, keeping only the non-allowed ones.
fn filter_capabilities(actual_caps: &CapabilitySet, allowed_caps: &CapabilitySet) -> CapabilitySet {
    if allowed_caps.contains(&Capability::Any) {
        CapabilitySet::default()
    } else if actual_caps.contains(&Capability::Any) {
        let mut result = CapabilitySet::new();
        result.insert(Capability::Any);
        result
    } else {
        actual_caps
            .iter()
            .filter(|cap| !allowed_caps.contains(cap))
            .copied()
            .collect()
    }
}

fn deduce_caps_of_binary(
    rules: &SymbolRules,
    path: &Utf8Path,
) -> anyhow::Result<DeducedCapabilities> {
    let symbols = crate::extract_symbols(path)?;
    let filtered_symbols = crate::filter_symbols(symbols, false, false);
    DeducedCapabilities::from_symbols(rules, filtered_symbols)
}
