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
    fn add_lib_or_bin(
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

fn make_cargo_command(args: &Args) -> Command {
    let mut cmd = Command::new("cargo");

    // Must be --quiet, or the output of cargo build will interfer with the output of cargo-caps.
    cmd.args(["build", "--quiet", "--message-format=json"]);

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
    cmd
}

#[derive(Parser)]
#[command(name = "cargo-caps")]
#[command(about = "A tool for analyzing capabilities")]
struct Args {
    /// Path to a specific rlib file to analyze (alternative to cargo build mode)
    rlib_path: Option<PathBuf>,

    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

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

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Direct rlib analysis mode
    if let Some(rlib_path) = &args.rlib_path {
        if !rlib_path.exists() {
            anyhow::bail!("Rlib file does not exist: {}", rlib_path.display());
        }

        let crate_name = rlib_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut caps = Caps {
            lib_caps: HashMap::new(),
        };

        let utf8_path = cargo_metadata::camino::Utf8PathBuf::from_path_buf(rlib_path.clone())
            .map_err(|p| anyhow::anyhow!("Invalid UTF-8 path: {}", p.display()))?;
        caps.add_lib_or_bin(crate_name, &utf8_path, args.verbose, None);
    } else {
        // Cargo build mode
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
                            caps.add_lib_or_bin(
                                &artifact.target.name,
                                file_path,
                                args.verbose,
                                Some(&artifact.features),
                            );
                        }
                    }
                }
            }
        }

        child.wait()?;

        println!();
        println!(
            "Run with -v/--verbose to get details about each dependency, or run `cargo-caps` with the path to a specific .rlib or binary to leaen more about it."
        );
    }

    Ok(())
}
