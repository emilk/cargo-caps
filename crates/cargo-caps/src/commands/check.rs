use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader},
    process::{Command, Stdio},
};

use anyhow::Context as _;
use cargo_metadata::{
    CargoOpt, Message, Metadata, MetadataCommand, PackageId, camino::Utf8PathBuf,
    diagnostic::DiagnosticLevel,
};
use itertools::Itertools as _;

use crate::{
    build_graph_analysis::DepKindSet,
    cap_rule::SymbolRules,
    checker::{Checker, CheckerOutput},
    config::WorkspaceConfig,
};

#[derive(clap::Parser)]
pub struct CheckCommand {
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

    /// Show crates with no capabilities after filtering
    #[arg(long = "show-empty")]
    pub show_empty: bool,

    /// Where to load the config file for the current workspace
    #[arg(long = "config", default_value = "cargo-caps.eon")]
    pub config: Utf8PathBuf,
}

impl CheckCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        let config = if self.config.exists() {
            WorkspaceConfig::from_path(&self.config)?
        } else {
            println!(
                "Expected config at {:?} - create one with 'cargo-caps init' or change the path with --config",
                self.config
            );
            println!();
            WorkspaceConfig::allow_basics()
        };

        let metadata = self.gather_cargo_metadata()?;
        let crate_infos = self.calc_crate_kinds(&metadata)?;

        // TODO: before starting the actual build,
        // make sure all build.rs files are allow-listed
        // or we might be in danger!

        let mut cmd = self.make_cargo_command();

        let verbose = self.verbose;

        let mut child = cmd.stdout(Stdio::piped()).spawn()?;

        let stdout = child.stdout.take().context("Failed to capture stdout")?;
        let reader = BufReader::new(stdout);

        let checker = Checker {
            rules: SymbolRules::load_default(),
            config,
            metadata,
            show_empty: self.show_empty,
        };
        let mut output = CheckerOutput::default();

        for line in reader.lines() {
            let line = line?;
            if let Ok(message) = serde_json::from_str::<Message>(&line) {
                match message {
                    Message::CompilerArtifact(artifact) => {
                        checker
                            .analyze_artifact(&mut output, &crate_infos, verbose, &artifact)
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
                        if false {
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

        if 0 < output.num_artifacts_passed {
            println!();
            println!(
                "{} artifact(s) passed the check",
                output.num_artifacts_passed
            );
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
    ) -> anyhow::Result<HashMap<PackageId, DepKindSet>> {
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

    fn cargo_toml_path_of_package(crate_name: &str) -> anyhow::Result<Utf8PathBuf> {
        let metadata = MetadataCommand::new()
            .manifest_path("./Cargo.toml")
            .features(CargoOpt::AllFeatures)
            .exec()?;

        // Search through workspace members
        for package in &metadata.workspace_packages() {
            if package.name.as_str() == crate_name {
                return Ok(package.manifest_path.clone());
            }
        }
        anyhow::bail!("Failed to locate manifest path of package '{crate_name}'");
    }

    fn gather_cargo_metadata(&self) -> anyhow::Result<cargo_metadata::Metadata> {
        let mut metadata_cmd = MetadataCommand::new();
        if let Some(package) = &self.package {
            metadata_cmd.manifest_path(Self::cargo_toml_path_of_package(package)?);
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
