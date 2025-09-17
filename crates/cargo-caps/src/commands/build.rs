use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader},
    process::{Command, Stdio},
};

use anyhow::Context as _;
use cargo_metadata::{Message, Metadata, MetadataCommand, PackageId, diagnostic::DiagnosticLevel};
use itertools::Itertools as _;

use crate::{
    Capability, CapabilitySet,
    analyzer::{CapsAnalyzer, CrateInfo},
};

#[derive(clap::Parser)]
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
                        println!("Warning: unknown capability '{s}' in ignored-caps"); // TODO: error
                    }
                    None
                }
            }
        })
        .collect()
}

impl BuildCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        let metadata = self.gather_cargo_metadata()?;

        // TODO: before starting the actual build,
        // make sure all build.rs files are allow-listed
        // or we might be in danger!

        let crate_infos = match self.calc_crate_kinds(&metadata) {
            Ok(crate_infos) => Some(crate_infos),
            Err(err) => {
                println!(
                    "Failed to analyze crate graph. cargo-deps won't understand if a dependency is a build-dependency, a dev-dependency, etc. Error: {err}"
                );
                println!();
                None
            }
        };

        let ignored_caps = parse_ignored_caps(&self.ignored_caps);

        let mut cmd = self.make_cargo_command();

        let verbose = self.verbose;

        let mut child = cmd.stdout(Stdio::piped()).spawn()?;

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut analyzer = CapsAnalyzer::new(ignored_caps.clone(), self.show_empty);

        for line in reader.lines() {
            let line = line?;
            if let Ok(message) = serde_json::from_str::<Message>(&line) {
                match message {
                    Message::CompilerArtifact(artifact) => {
                        analyze_artifact(
                            &metadata,
                            &mut analyzer,
                            crate_infos.as_ref(),
                            verbose,
                            &artifact,
                        )
                        .with_context(|| format!("target name: {}", artifact.target.name))?;
                    }
                    Message::CompilerMessage(compiler_message) => {
                        let show = !matches!(
                            compiler_message.message.level,
                            DiagnosticLevel::Warning
                                | DiagnosticLevel::Note
                                | DiagnosticLevel::Help
                        );
                        if show {
                            println!("CompilerMessage: {compiler_message}");
                        }
                    }
                    Message::BuildScriptExecuted(build_script) => {
                        if true {
                            // TODO: figure out the path of the binary so we can analyze the symbls in it
                        } else {
                            println!("BuildScriptExecuted: {build_script:?}");
                        }
                    }
                    Message::BuildFinished(build_finished) => {
                        if build_finished.success {
                            println!("Build finished successfully");
                        } else {
                            println!("Build failed"); // TODO: return error
                        }
                    }
                    Message::TextLine(text_line) => {
                        println!("TextLine: {text_line}");
                    }
                    _ => {}
                }
            }
        }

        child.wait()?;

        if 0 < analyzer.num_skipped {
            println!();

            if ignored_caps.is_empty() {
                println!(
                    "Skipped printing {} crate(s) that had zero capabilities",
                    analyzer.num_skipped
                );
            } else {
                println!(
                    "Skipped printing {} crate(s) that only had the following capabilities (or less): {}",
                    analyzer.num_skipped,
                    ignored_caps.iter().join(", ")
                );
            }
            println!("(You can control this with --ignored-caps)");
        }

        println!();
        println!(
            "Run with -v/--verbose to get details about each dependency, or run `cargo-caps caps` with the path to a specific binary (executable, .rlib, .dylib, …) to learn more about it."
        );

        Ok(())
    }

    fn calc_crate_kinds(
        &self,
        metadata: &Metadata,
    ) -> anyhow::Result<HashMap<PackageId, CrateInfo>> {
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

        let sources = target_packages.iter().map(|p| p.id.clone()).collect_vec();
        crate::build_graph_analysis::analyze_dependency_graph(metadata, &sources)
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
    metadata: &Metadata,
    analyzer: &mut CapsAnalyzer,
    crate_infos: Option<&HashMap<PackageId, CrateInfo>>,
    verbose: bool,
    artifact: &cargo_metadata::Artifact,
) -> Result<(), anyhow::Error> {
    let package = metadata
        .packages
        .iter()
        .find(|p| p.id == artifact.package_id)
        .unwrap(); // TODO

    // TODO: all TargetKind?
    // if artifact.target.kind.iter().any(|k| k == &TargetKind::Lib)
    {
        // let name = &artifact.target.name;
        let crate_info = if let Some(crate_infos) = crate_infos {
            if let Some(crate_info) = crate_infos.get(&artifact.package_id) {
                Some(crate_info)
            } else {
                // Not sure why we sometimes end up here.
                // Examples: bitflags block2 objc2 objc2_app_kit
                // anyhow::bail!("ERROR: unknown crate {name:?}");
                println!("ERROR: unknown crate {}", artifact.target.name); // TODO: continue, then exit with error
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
                    analyzer.add_artifact(package, artifact, file_path, crate_info, verbose)?;
                if !did_print {
                    analyzer.num_skipped += 1;
                }
            }
        }
    }
    // else if artifact
    //     .target
    //     .kind
    //     .iter()
    //     .any(|k| k == &TargetKind::CustomBuild)
    // {
    //     // build.rs
    // } else {
    //     println!(
    //         "Ignoring artifact {} of kind {:?}",
    //         artifact.target.name, artifact.target.kind
    //     );
    // }
    Ok(())
}
