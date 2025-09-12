use std::{
    io::{BufRead as _, BufReader},
    process::{Command, Stdio},
};

use cargo_metadata::{Message, TargetKind};
use clap::Parser;

use crate::analyzer::CapsAnalyzer;

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
    #[arg(long = "ignored-caps", default_value = "alloc,panic")]
    pub ignored_caps: String,

    /// Show crates with no capabilities after filtering
    #[arg(long = "show-empty")]
    pub show_empty: bool,
}

impl BuildCommand {
    pub fn execute(&self) -> anyhow::Result<()> {
        // Inform user about ignored capabilities if any are specified
        if !self.ignored_caps.is_empty() {
            println!("ignored-caps: {}", self.ignored_caps);
            println!();
        }

        let mut cmd = self.make_cargo_command();

        let mut child = cmd.stdout(Stdio::piped()).spawn()?;

        let stdout = child.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut analyzer = CapsAnalyzer::new(&self.ignored_caps, self.show_empty);

        for line in reader.lines() {
            let line = line?;
            if let Ok(message) = serde_json::from_str::<Message>(&line)
                && let Message::CompilerArtifact(artifact) = message
            {
                // Filter for library artifacts
                if artifact.target.kind.iter().any(|k| k == &TargetKind::Lib) {
                    for file_path in &artifact.filenames {
                        if file_path.as_str().ends_with(".rlib") {
                            analyzer.add_lib_or_bin(&artifact, file_path, self.verbose);
                        }
                    }
                }
            }
        }

        child.wait()?;

        println!();
        println!(
            "Run with -v/--verbose to get details about each dependency, or run `cargo-caps caps` with the path to a specific .rlib or binary to learn more about it."
        );

        Ok(())
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
