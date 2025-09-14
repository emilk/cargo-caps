use std::{
    collections::{HashMap, HashSet},
    io::{BufRead as _, BufReader},
    process::{Command, Stdio},
};

use anyhow::Context as _;
use cargo_metadata::{DependencyKind, Message, MetadataCommand, Package, PackageId, TargetKind};
use clap::Parser;
use itertools::Itertools as _;

use crate::{
    Capability, CapabilitySet,
    analyzer::{CapsAnalyzer, CrateInfo, CrateKind},
};

#[derive(Parser)]
/// Analyze capabilities by running cargo build
pub struct BuildCommand {
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,

    #[arg(short = 'p', long = "package")]
    pub package: Option<String>,

    #[arg(short = 'F', long = "features")]
    pub features: Vec<String>,

    #[arg(long = "all-features")]
    pub all_features: bool,

    #[arg(long = "no-default-features")]
    pub no_default_features: bool,

    #[arg(long = "release")]
    pub release: bool,

    #[arg(short = 'q', long = "quiet")]
    pub quiet: bool,

    /// Capabilities to ignore when displaying results (comma-separated, lowercase)
    #[arg(long = "ignored-caps", default_value = "alloc,stdio,time,panic")]
    pub ignored_caps: String,

    /// Show crates with no capabilities after filtering
    #[arg(long = "show-empty")]
    pub show_empty: bool,
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
                "fas" => Some(Capability::FS),
                "any" => Some(Capability::Any),
                _ => {
                    if !s.is_empty() {
                        eprintln!("Warning: unknown capability '{s}' in ignored-caps"); // TODO: error
                    }
                    None
                }
            }
        })
        .collect()
}

impl BuildCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        // Analyze dependencies before building
        let crate_infos = self.analyze_dependencies()?;

        let ignored_caps = parse_ignored_caps(&self.ignored_caps);

        // Inform user about ignored capabilities if any are specified
        if !self.ignored_caps.is_empty() {
            println!("ignored-caps: {}", self.ignored_caps);
            println!();
        }

        let mut cmd = self.make_cargo_command();

        let verbose = self.verbose;

        let mut child = cmd.stdout(Stdio::piped()).spawn()?;

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut analyzer = CapsAnalyzer::new(ignored_caps.clone(), self.show_empty);

        for line in reader.lines() {
            let line = line?;
            if let Ok(message) = serde_json::from_str::<Message>(&line)
                && let Message::CompilerArtifact(artifact) = message
            {
                analyze_artifact(&mut analyzer, &crate_infos, verbose, &artifact)
                    .with_context(|| format!("Name: {}", artifact.target.name))?;
            }
        }

        child.wait()?;

        println!();
        println!(
            "Run with -v/--verbose to get details about each dependency, or run `cargo-caps caps` with the path to a specific .rlib or binary to learn more about it."
        );

        Ok(())
    }

    fn analyze_dependencies(&self) -> anyhow::Result<HashMap<PackageId, CrateInfo>> {
        let metadata = self.gather_cargo_metadata()?;

        // Get the package(s) we're interested in
        let target_packages = if let Some(package_name) = &self.package {
            metadata
                .packages
                .iter()
                .filter(|p| p.name.as_str() == package_name)
                .collect()
        } else {
            // If no specific package, analyze workspace members
            metadata.workspace_packages()
        };

        if true {
            let sources = target_packages.iter().map(|p| p.id.clone()).collect_vec();
            super::graph_analysis::analyze_dependency_dag(&metadata, &sources)
        } else {
            let package_map: HashMap<&PackageId, &Package> =
                metadata.packages.iter().map(|p| (&p.id, p)).collect();

            let mut crate_infos: HashMap<PackageId, CrateInfo> = HashMap::new();

            for package in target_packages {
                println!("Package: {}", package.name);

                crate_infos.entry(package.id.clone()).or_default(); // Remember all the top targets

                // Collect all transitive dependencies recursively
                let mut visited = HashSet::new();
                let mut all_deps = HashMap::new();

                collect_transitive_deps(&package.id, &package_map, &mut visited, &mut all_deps);

                #[expect(clippy::iter_over_hash_type)] // is ok: we sort the results
                for (pkg_id, dep_kinds) in &all_deps {
                    if let Some(pkg) = package_map.get(pkg_id) {
                        // Check if this is a proc-macro
                        let is_proc_macro = pkg
                            .targets
                            .iter()
                            .any(|t| t.kind.iter().any(|k| k == &TargetKind::ProcMacro));

                        let crate_info = crate_infos.entry(pkg.id.clone()).or_default();

                        if is_proc_macro {
                            crate_info.kind.insert(CrateKind::ProcMacro);
                        }

                        for dep_kind in dep_kinds {
                            let crate_kind = match dep_kind {
                                DependencyKind::Normal => CrateKind::Normal,
                                DependencyKind::Development => CrateKind::Dev,
                                DependencyKind::Build => CrateKind::Build,
                                DependencyKind::Unknown => CrateKind::Unknown,
                            };
                            crate_info.kind.insert(crate_kind);
                        }
                    }
                }
            }

            Ok(crate_infos)
        }
    }

    fn gather_cargo_metadata(&self) -> Result<cargo_metadata::Metadata, anyhow::Error> {
        let mut metadata_cmd = MetadataCommand::new();
        if let Some(_package) = &self.package {
            // For metadata, we need to specify the manifest path or current dir
            // The package filter will be applied when analyzing
        }
        if !self.features.is_empty() {
            metadata_cmd.features(cargo_metadata::CargoOpt::SomeFeatures(
                self.features.clone(),
            ));
        }
        if self.all_features {
            metadata_cmd.features(cargo_metadata::CargoOpt::AllFeatures);
        }
        if self.no_default_features {
            metadata_cmd.features(cargo_metadata::CargoOpt::NoDefaultFeatures);
        }
        let metadata = metadata_cmd.exec()?;
        Ok(metadata)
    }

    fn make_cargo_command(&self) -> Command {
        let mut cmd = Command::new("cargo");

        // Must be --quiet, or the output of cargo build will interfere with the output of cargo-caps.
        cmd.args(["build", "--quiet", "--message-format=json"]);

        if let Some(package) = &self.package {
            cmd.args(["-p", package]);
        }

        if !self.features.is_empty() {
            cmd.args(["-F", &self.features.join(",")]);
        }

        if self.all_features {
            cmd.arg("--all-features");
        }

        if self.no_default_features {
            cmd.arg("--no-default-features");
        }

        if self.release {
            cmd.arg("--release");
        }
        cmd
    }
}

fn analyze_artifact(
    analyzer: &mut CapsAnalyzer,
    crate_infos: &HashMap<PackageId, CrateInfo>,
    verbose: bool,
    artifact: &cargo_metadata::Artifact,
) -> Result<(), anyhow::Error> {
    // TODO: all TargetKind?
    if artifact.target.kind.iter().any(|k| k == &TargetKind::Lib) {
        let name = &artifact.target.name;
        let crate_info = crate_infos.get(&artifact.package_id);
        if let Some(crate_info) = crate_info {
            for file_path in &artifact.filenames {
                if file_path.as_str().ends_with(".rlib") {
                    analyzer.add_lib_or_bin(artifact, crate_info, file_path, verbose)?;
                }
            }
        } else {
            // Not sure why we sometimes end up here.
            // Examples: bitflags block2 objc2 objc2_app_kit
            // anyhow::bail!("ERROR: unknown crate {name:?}");
            eprintln!("ERROR: unknown crate {name:?}"); // TODO: continue, then exit with error
        }
    }
    Ok(())
}

fn collect_transitive_deps(
    package_id: &PackageId,
    package_map: &HashMap<&PackageId, &Package>,
    visited: &mut HashSet<PackageId>,
    all_deps: &mut HashMap<PackageId, HashSet<DependencyKind>>,
) {
    // Avoid infinite recursion
    if visited.contains(package_id) {
        return;
    }
    visited.insert(package_id.clone());

    if let Some(package) = package_map.get(package_id) {
        for dep in &package.dependencies {
            // Find the actual package for this dependency
            if let Some(dep_package) = package_map
                .values()
                .find(|p| p.name.as_str() == dep.name.as_str())
            {
                // Record this dependency with its kind
                all_deps
                    .entry(dep_package.id.clone())
                    .or_default()
                    .insert(dep.kind);

                // TODO: if this is ONLY a build dependency, we should mark everything below as ONLY build dependency.
                // We probably need pet-graph for this.
                collect_transitive_deps(&dep_package.id, package_map, visited, all_deps);
            } else {
                // I think we get here for dependencies that are disabled for this feature set
                // eprintln!("ERROR: failed to find package {:?}", dep.name);
            }
        }
    }
}
