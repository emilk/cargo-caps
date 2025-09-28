use std::collections::{BTreeMap, HashMap};

use crate::{
    CrateName,
    build_graph_analysis::{DepKind, DepKindSet, has_build_rs},
    cap_rule::SymbolRules,
    capability::{Capability, CapabilitySet, DeducedCaps, Reason, format_reasons},
    config::WorkspaceConfig,
    src_analysis::ParsedRust,
};
use cargo_metadata::{
    Artifact, DependencyKind, Metadata, Package, PackageId, TargetKind, camino::Utf8Path,
};
use itertools::Itertools as _;

/// What [`Checker`] computers
#[derive(Default)]
pub struct CheckerOutput {
    pub crate_caps: HashMap<CrateName, BTreeMap<TargetKind, DeducedCaps>>,
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
        crate_infos: &HashMap<PackageId, DepKindSet>,
        verbose: bool,
        artifact: &cargo_metadata::Artifact,
    ) -> anyhow::Result<()> {
        if artifact.executable.is_some() {
            // When building a workspace there is a lot of example binaries etc.
            // They all have all the capabilities.
            // NOTE: this does NOT skip build.rs files.
            return Ok(());
        }

        let package = self
            .metadata
            .packages
            .iter()
            .find(|p| p.id == artifact.package_id)
            .unwrap(); // TODO

        let set = if let Some(set) = crate_infos.get(&artifact.package_id) {
            // TODO
            // if !set.kind.contains(&DepKind::Normal) {
            //     return Ok(()); // ignore build dependencies, proc-macros etc - they cannot affect users machines
            // }

            set
        } else {
            // Not sure why we sometimes end up here.
            // Examples: bitflags block2 objc2 objc2_app_kit memoffset rustix
            println!("ERROR: unknown crate {}", artifact.target.name);
            return Ok(());
            // None
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

    fn deduce_caps(
        &self,
        output: &CheckerOutput,
        package: &Package,
        artifact: &Artifact,
        bin_path: &Utf8Path,
    ) -> anyhow::Result<DeducedCaps> {
        let crate_name = CrateName::new(package.name.to_string())?;

        debug_assert_eq!(
            artifact.target.kind.len(),
            1,
            "Expected a single, kind, got {:?}",
            artifact.target.kind
        );
        let artifact_kind = &artifact.target.kind[0];

        let mut deduced_caps = if matches!(
            artifact_kind,
            &TargetKind::CustomBuild | &TargetKind::ProcMacro
        ) {
            // build.rs files and proc-macros are binaries with a main function and everything.
            // There is very little they can't do.
            // So they will always be sus
            let artifact_kind_name = match artifact_kind {
                TargetKind::CustomBuild => "build.rs",
                TargetKind::ProcMacro => "proc-macro",
                _ => unreachable!(),
            };

            match ParsedRust::parse_file(&artifact.target.src_path) {
                Ok(parsed) => {
                    let ParsedRust { all_paths, capabilities: _ } = parsed;
                    DeducedCaps::from_paths(&self.rules, all_paths.into_iter())?
                }
                Err(err) => {
                    let mut deduced_caps = DeducedCaps::default();
                    deduced_caps.caps.insert(
                        Capability::Unknown,
                        std::iter::once(Reason::SourceParseError(format!("{err:#}"))).collect(),
                    );
                    deduced_caps
                }
            }
        } else {
            deduce_caps_of_binary(&self.rules, bin_path)?
        };

        // Extend capabilities with the capabilities of our actual dependencies.
        // TODO: we do it again below, but differently
        for (dep_crate_name, _) in std::mem::take(&mut deduced_caps.unresolved_crates) {
            if dep_crate_name == crate_name {
                continue; // A crate can depend on itself
            }
            if let Some(crate_caps) = output.crate_caps.get(&dep_crate_name) {
                if let Some(dep_caps) = crate_caps.get(&TargetKind::Lib) {
                    // If a dependency has a capability, then so do we!
                    for &cap in dep_caps.caps.keys() {
                        deduced_caps
                            .caps
                            .entry(cap)
                            .or_default()
                            .insert(Reason::Crate(dep_crate_name.clone()));
                    }
                } else {
                    // TODO: return error?
                    println!(
                        "{crate_name} depends on '{dep_crate_name}' (according to cargo-caps), but we have no Lib capabilities stored for it, only {:?}",
                        crate_caps.keys()
                    );
                }
            } else {
                // We end up here for crates that produce no binaries, like `vec1`
                // println!(
                //     "{crate_name} depends on '{dep_crate_name}' (according to cargo-caps), but we have no knows capabilities for it"
                // );
            }
        }

        // Extend capabilities with the capabilities of our supposed dependencies.
        // TODO: we do it already above, but differently
        let resolve = self.metadata.resolve.as_ref().unwrap();
        let node = resolve
            .nodes
            .iter()
            .find(|node| node.id == package.id)
            .unwrap();
        for dependency in &node.deps {
            if !dependency
                .dep_kinds
                .iter()
                .any(|kind| kind.kind == DependencyKind::Normal)
            {
                let dep_crate_name = CrateName::new(dependency.name.clone())?;
                if let Some(crate_caps) = output.crate_caps.get(&dep_crate_name) {
                    if let Some(dep_caps) = crate_caps.get(&TargetKind::Lib) {
                        // If a dependency has a capability, then so do we!
                        for &cap in dep_caps.caps.keys() {
                            deduced_caps
                                .caps
                                .entry(cap)
                                .or_default()
                                .insert(Reason::Crate(dep_crate_name.clone()));
                        }
                    } else {
                        // TODO: return error?
                        println!(
                            "{crate_name} depends on '{dep_crate_name}' (according to cargo-metadata), but we have no Lib capabilities stored for it, only {:?}",
                            crate_caps.keys()
                        );
                    }
                } else {
                    // TODO: figure out why we sometimes end up here
                    println!(
                        "{crate_name} depends on '{dep_crate_name}' (according to cargo-metadata) which we haven't compiled"
                    );
                }
            }
        }

        if deduced_caps.caps.keys().any(Capability::is_critical) {
            // If we have critical capabilities, all the others are uninteresting
            deduced_caps.caps.retain(|key, _| key.is_critical());
        }

        Ok(deduced_caps)
    }

    /// NOTE: each crate can have multiple artifacts, e.g. both a `custom-build` (build.rs)
    /// and a library.
    ///
    /// Returns `true` if we printed anything
    fn add_artifact(
        &self,
        output: &mut CheckerOutput,
        package: &Package,
        artifact: &Artifact,
        bin_path: &Utf8Path,
        dep_kinds: &DepKindSet,
        verbose: bool,
    ) -> anyhow::Result<bool> {
        let crate_name = CrateName::new(package.name.to_string())?;

        let mut deduced_caps = self.deduce_caps(output, package, artifact, bin_path)?;

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
            deduced_caps.caps.entry(Capability::BuildRs).or_default();
        }

        let allowed_caps = self.config.crate_caps(&crate_name);

        let crate_kind_suffix = {
            if artifact.target.kind.contains(&TargetKind::CustomBuild) {
                " (build.rs)".to_owned()
            } else if artifact.target.kind.contains(&TargetKind::ProcMacro) {
                " (proc-macro)".to_owned()
            } else if artifact.target.kind.contains(&TargetKind::Bin) {
                " (bin)".to_owned()
            } else if dep_kinds.kind.contains(&DepKind::Normal) {
                String::new() // Not worth mentioning
            } else {
                format!(" ({})", dep_kinds.kind.iter().join(", "))
            }
        };

        let criticals = deduced_caps
            .caps
            .iter()
            .filter(|(c, _)| c.is_critical())
            .map(|(c, reasons)| format!("{} {c} because of {}", c.emoji(), format_reasons(reasons)))
            .collect_vec();

        let info = if !criticals.is_empty() {
            criticals.join(", ")
        } else {
            let filtered_caps = filter_capabilities(&deduced_caps, &allowed_caps);

            if filtered_caps.is_empty() {
                if self.show_empty {
                    "😌 none".to_owned()
                } else {
                    return Ok(false); // TODO: respect verbose? maybe?
                }
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

            println!(
                "  Artifact kind: {}",
                artifact.target.kind.iter().join(", ")
            );
            println!("  Crate kind: {}", dep_kinds.kind.iter().join(", "));

            if !artifact.target.kind.contains(&TargetKind::CustomBuild) && has_build_rs(package) {
                let build_rs_caps = output
                    .crate_caps
                    .get(&crate_name)
                    .and_then(|crate_caps| crate_caps.get(&TargetKind::CustomBuild));
                if let Some(build_rs_caps) = build_rs_caps {
                    println!(
                        "  {crate_name} build.rs capabilities: {}",
                        build_rs_caps.caps.keys().join(", ")
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
fn filter_capabilities(actual_caps: &DeducedCaps, allowed_caps: &CapabilitySet) -> CapabilitySet {
    let actual_caps: CapabilitySet = actual_caps.caps.keys().copied().collect();

    if allowed_caps.contains(&Capability::Wildcard) {
        CapabilitySet::default()
    } else {
        actual_caps
            .iter()
            .filter(|cap| !allowed_caps.contains(cap))
            .copied()
            .collect()
    }
}

fn deduce_caps_of_binary(rules: &SymbolRules, path: &Utf8Path) -> anyhow::Result<DeducedCaps> {
    let symbols = crate::extract_symbols(path)?;
    let filtered_symbols = crate::filter_symbols(symbols, false, false);
    DeducedCaps::from_symbols(rules, filtered_symbols)
}
