use std::{
    collections::HashMap,
    io::{BufRead as _, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use capabara::capability::DeducedCapablities;
use cargo_metadata::{Message, TargetKind};
use clap::Parser;

struct Caps {
    lib_caps: HashMap<String, DeducedCapablities>,
}

impl Caps {
    fn add_lib_or_bin(&mut self, crate_name: &str, bin_path: &cargo_metadata::camino::Utf8PathBuf) {
        let path = PathBuf::from(bin_path.as_str());

        // Analyze capabilities for this rlib
        let Some(mut deduced_caps) = deduce_caps_of_binary(&path) else {
            eprintln!("ERROR: failed to decude capabilities of {bin_path:?}"); // TODO: report error
            return;
        };

        deduced_caps.unknown_crates.remove(crate_name);

        deduced_caps.unknown_crates.retain(|dep_crate_name, _| {
            if let Some(dep_caps) = self.lib_caps.get(dep_crate_name) {
                deduced_caps
                    .known_crates
                    .entry(dep_crate_name.clone())
                    .or_default()
                    .extend(dep_caps.total_capabilities());
                false // no longer unknown
            } else {
                true // still unknown
            }
        });

        // Print short description
        let cap_names: Vec<String> = deduced_caps
            .total_capabilities()
            .iter()
            .map(|c| format!("{c:?}"))
            .collect();
        let cap_list = if cap_names.is_empty() {
            "none".to_owned()
        } else {
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
            warnings.push(format!(
                "{} unknown symbols",
                deduced_caps.unknown_symbols.len()
            ));
        }

        let warning_text = if warnings.is_empty() {
            String::new()
        } else {
            format!(" ⚠️ {}", warnings.join(", "))
        };

        println!("{crate_name}: [{cap_list}]{warning_text}");

        self.lib_caps.insert(crate_name.to_owned(), deduced_caps);
    }
}

fn make_cargo_command(args: &Args) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--message-format=json"]);

    if let Some(package) = &args.package {
        cmd.args(["-p", package]);
    }

    if !args.features.is_empty() {
        cmd.args(["-F", &args.features.join(",")]);
    }

    if args.all_features {
        cmd.arg("--all-features");
    }

    if args.no_default_features {
        cmd.arg("--no-default-features");
    }

    if args.release {
        cmd.arg("--release");
    }

    if args.quiet {
        cmd.arg("--quiet");
    }
    cmd
}

#[derive(Parser)]
#[command(name = "cargo-caps")]
#[command(about = "A tool for analyzing capabilities")]
struct Args {
    #[arg(short = 'p', long = "package")]
    package: Option<String>,

    #[arg(short = 'F', long = "features")]
    features: Vec<String>,

    #[arg(long = "all-features")]
    all_features: bool,

    #[arg(long = "no-default-features")]
    no_default_features: bool,

    #[arg(long = "release")]
    release: bool,

    #[arg(short = 'q', long = "quiet")]
    quiet: bool,
}

fn deduce_caps_of_binary(path: &Path) -> Option<DeducedCapablities> {
    let symbols = capabara::extract_symbols(path).ok()?;
    let filtered_symbols = capabara::filter_symbols(symbols, false, false);
    Some(DeducedCapablities::from_symbols(filtered_symbols))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let mut cmd = make_cargo_command(&args);

    let mut child = cmd.stdout(Stdio::piped()).spawn()?;

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    let mut caps = Caps {
        lib_caps: HashMap::new(),
    };

    for line in reader.lines() {
        let line = line?;
        if let Ok(message) = serde_json::from_str::<Message>(&line)
            && let Message::CompilerArtifact(artifact) = message
        {
            // Filter for library artifacts
            if artifact.target.kind.iter().any(|k| k == &TargetKind::Lib) {
                for file_path in &artifact.filenames {
                    if file_path.as_str().ends_with(".rlib") {
                        caps.add_lib_or_bin(&artifact.target.name, file_path);
                    }
                }
            }
        }
    }

    child.wait()?;

    Ok(())
}
