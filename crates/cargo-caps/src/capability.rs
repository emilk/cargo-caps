use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use anyhow::Context as _;
use cargo_metadata::camino::Utf8PathBuf;
use itertools::Itertools as _;
use serde::{Deserialize, Serialize};

use crate::{
    CrateName, Symbol, cap_rule::SymbolRules, reservoir_sample::ReservoirSampleExt as _,
    rust_path::RustPath, symbol::FunctionOrPath,
};

/// A set of capabilities.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    pub fn insert(&mut self, cap: Capability) -> bool {
        self.0.insert(cap)
    }

    pub fn contains(&self, cap: &Capability) -> bool {
        self.0.contains(cap)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Capability> {
        self.0.iter()
    }

    pub fn difference<'a>(&'a self, other: &'a Self) -> impl Iterator<Item = &'a Capability> {
        self.0.difference(&other.0)
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self(BTreeSet::from_iter(iter))
    }
}

impl Extend<Capability> for CapabilitySet {
    fn extend<T: IntoIterator<Item = Capability>>(&mut self, iter: T) {
        self.0.extend(iter);
    }
}

impl<'a> IntoIterator for &'a CapabilitySet {
    type Item = &'a Capability;
    type IntoIter = std::collections::btree_set::Iter<'a, Capability>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// A capability a crate can be granted,
/// or is suspected of having.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    /// This crate has a custom build step (build.rs)
    ///
    /// NOT contagious!
    /// Depending on a crate with a build.rs file does not give you the `BuildRs` capability.
    #[serde(rename = "build.rs")]
    BuildRs,

    /// Allocate memory (`Box::new`, `Vec::new`, …)
    #[serde(rename = "alloc")]
    Alloc,

    /// Call [`panic!`]
    #[serde(rename = "panic")]
    Panic,

    /// Read the current time and/or date
    #[serde(rename = "time")]
    Time,

    /// Read environment variables, process info, …
    #[serde(rename = "sysinfo")]
    Sysinfo,

    /// Read and write to stdin, stdout, stderr
    #[serde(rename = "stdio")]
    Stdio,

    /// Spawn thread
    #[serde(rename = "thread")]
    Thread,

    /// Connect over the network and/or listen for incoming network traffic
    #[serde(rename = "net")]
    Net,

    /// Open a file on disk for reading or writing
    #[serde(rename = "fs")]
    FS,

    // -------------------------------
    // Dangerous ones:
    /// Contains unsafe code blocks or functions
    #[serde(rename = "unsafe")]
    Unsafe,

    /// May call any CLI command
    #[serde(rename = "command")]
    Command,

    /// We don't know
    #[serde(rename = "unknown")]
    Unknown,

    /// Only used as an "allow" rule
    #[serde(rename = "*")]
    Wildcard,
}

impl Capability {
    /// Any capability that is "critical" could theoretically lead to all other capapbilities.
    pub fn is_critical(&self) -> bool {
        match self {
            Self::BuildRs
            | Self::Alloc
            | Self::Panic
            | Self::Time
            | Self::Sysinfo
            | Self::Stdio
            | Self::Thread
            | Self::Net
            | Self::FS => false,

            Self::Unsafe | Self::Command | Self::Unknown | Self::Wildcard => true,
        }
    }

    /// Should we inherit this capability if a dependency has it?
    pub fn inherit_from_dependency(&self) -> bool {
        // Motivation: all critical dependencies need explicit vetting,
        // and once vetted they _should_ shrink from e.g. "unknown" or "unsafe" to something actually known.
        !self.is_critical()
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BuildRs => write!(f, "build.rs"),
            Self::Alloc => write!(f, "alloc"),
            Self::Panic => write!(f, "panic"),
            Self::Time => write!(f, "time"),
            Self::Sysinfo => write!(f, "sysinfo"),
            Self::Stdio => write!(f, "stdio"),
            Self::Thread => write!(f, "thread"),
            Self::Net => write!(f, "net"),
            Self::FS => write!(f, "fs"),
            Self::Unsafe => write!(f, "unsafe"),
            Self::Command => write!(f, "command"),
            Self::Unknown => write!(f, "unknown"),
            Self::Wildcard => write!(f, "*"),
        }
    }
}

impl Capability {
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::BuildRs => "🛠️ ",
            Self::Alloc => "📦",
            Self::Panic => "❗️",
            Self::Time => "⏰",
            Self::Sysinfo => "🖥️ ",
            Self::Stdio => "📝",
            Self::Thread => "🧵",
            Self::Net => "🌐",
            Self::FS => "📁",
            Self::Unsafe => "☢️",
            Self::Command => "⚠️ ",
            Self::Unknown => "❓",
            Self::Wildcard => "🃏 ", // TODO: its own symbol?
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct DeducedCaps {
    /// The capabilities of this crate
    pub caps: BTreeMap<Capability, Reasons>,

    /// We need to resolve these crates to see what their capabilities are.
    ///
    /// The value of the map is what indicated that we were using this crate in the first place.
    pub unresolved_crates: BTreeMap<CrateName, BTreeSet<RustPath>>,
}

/// Why do we have this capability?
pub type Reasons = BTreeSet<Reason>;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reason {
    /// This path matches a rule. TODO: which rule? Where?
    PathMatchedRule(RustPath),

    /// This symbol matches a rule. TODO: which rule? Where?
    SymbolMatchedRule(Symbol),

    /// The reason we have this high capability is because we didn't succeed in understanding the source code.
    SourceParseError(String),

    /// Because of analysing the source code.
    // TODO: add path, file, line number
    SourceCodeAnalysis { location: SourceLocation },

    /// The reason we have this high capability is because we couldn't match this symbol to any rule.
    ///
    /// If you hit this, you need to extend `default_rules.eon`
    UnmatchedSymbol(Symbol),

    /// Path to `alloc`, `core`, or `std` that didn't match any rule in `default_rules.eon`.
    UmatchedStandardPath(RustPath),

    /// We have this capability because we depend on this crate, which has that capability.
    Crate(CrateName),
}

impl std::fmt::Display for Reason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PathMatchedRule(path) | Self::UmatchedStandardPath(path) => path.fmt(f),
            Self::SourceParseError(err) => write!(f, "{err:#?}"),
            Self::SymbolMatchedRule(symbol) | Self::UnmatchedSymbol(symbol) => {
                write!(f, "{}", symbol.format(false))
            }
            Self::SourceCodeAnalysis { location } => write!(f, "source: {location}"),
            Self::Crate(crate_name) => crate_name.fmt(f),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceLocation {
    pub path: Arc<Utf8PathBuf>,
    pub line_nr: usize,
}

impl std::fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self { path, line_nr } = self;
        write!(f, "{path}:{line_nr}")
    }
}

impl DeducedCaps {
    pub fn from_symbols(
        rules: &SymbolRules,
        symbols: impl IntoIterator<Item = Symbol>,
    ) -> anyhow::Result<Self> {
        let mut slf = Self::default();
        for symbol in symbols {
            slf.add_symbol(rules, &symbol)?;
        }
        Ok(slf)
    }

    /// Capability from symbol
    fn add_symbol(&mut self, rules: &SymbolRules, symbol: &Symbol) -> anyhow::Result<()> {
        for path in symbol.paths() {
            match path {
                FunctionOrPath::Function(fun_name) => {
                    let fun_name = fun_name.trim_start_matches('_');

                    // Check rules for the symbol
                    if let Some(capabilities) = rules.match_symbol(fun_name) {
                        for &capability in capabilities {
                            self.caps
                                .entry(capability)
                                .or_default()
                                .insert(Reason::SymbolMatchedRule(symbol.clone()));
                        }
                    } else {
                        self.caps
                            .entry(Capability::Unknown)
                            .or_default()
                            .insert(Reason::UnmatchedSymbol(symbol.clone()));
                    }
                }

                FunctionOrPath::RustPath(rust_path) => {
                    let path_str = rust_path.to_string();
                    // Check rules for the path
                    if let Some(capabilities) = rules.match_symbol(&path_str) {
                        for &capability in capabilities {
                            self.caps
                                .entry(capability)
                                .or_default()
                                .insert(Reason::PathMatchedRule(rust_path.clone()));
                        }
                    } else {
                        // No rule matched

                        let segments = rust_path.segments();
                        let crate_name = CrateName::new(segments[0])
                            .with_context(|| format!("mangled: {:?}", symbol.mangled))
                            .with_context(|| format!("demangled: {:?}", symbol.demangled))?;

                        if crate_name.is_standard_crate() {
                            self.caps
                                .entry(Capability::Unknown)
                                .or_default()
                                .insert(Reason::UmatchedStandardPath(rust_path.clone()));
                        } else {
                            // assume an external crate:
                            self.unresolved_crates
                                .entry(crate_name)
                                .or_default()
                                .insert(rust_path);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub fn add_path(&mut self, rules: &SymbolRules, rust_path: RustPath) -> anyhow::Result<()> {
        let path_str = rust_path.to_string();
        // Check rules for the path
        if let Some(capabilities) = rules.match_symbol(&path_str) {
            for &capability in capabilities {
                self.caps
                    .entry(capability)
                    .or_default()
                    .insert(Reason::PathMatchedRule(rust_path.clone()));
            }
        } else {
            // No rule matched - assume an external crate:
            let segments = rust_path.segments();

            let crate_name = segments[0];
            let crate_name =
                CrateName::new(crate_name).with_context(|| format!("path: {rust_path}"))?;
            self.unresolved_crates
                .entry(crate_name)
                .or_default()
                .insert(rust_path);
        }

        Ok(())
    }

    pub fn extend(&mut self, other: Self) {
        let Self {
            caps,
            unresolved_crates,
        } = self;
        for (cap, reasons) in other.caps {
            caps.entry(cap).or_default().extend(reasons);
        }
        for (crate_name, reasons) in other.unresolved_crates {
            unresolved_crates
                .entry(crate_name)
                .or_default()
                .extend(reasons);
        }
    }
}

pub fn format_reasons(reasons: &Reasons) -> String {
    let mut crates = vec![];
    let mut path_matched_rules = vec![];
    let mut symbol_matched_rules = vec![];
    let mut unmatched_paths = vec![];
    let mut unmatched_symbols = vec![];
    let mut source_parse_errors = vec![];
    let mut source_code_locations = vec![];

    for reason in reasons {
        match reason {
            Reason::Crate(crate_name) => {
                crates.push(crate_name);
            }
            Reason::UmatchedStandardPath(path) => {
                unmatched_paths.push(path);
            }
            Reason::UnmatchedSymbol(symbol) => {
                unmatched_symbols.push(symbol);
            }
            Reason::PathMatchedRule(rust_path) => {
                path_matched_rules.push(rust_path);
            }
            Reason::SymbolMatchedRule(symbol) => {
                symbol_matched_rules.push(symbol);
            }
            Reason::SourceParseError(error) => {
                source_parse_errors.push(error);
            }
            Reason::SourceCodeAnalysis { location } => {
                source_code_locations.push(location);
            }
        }
    }

    fn format_long_list<T: std::fmt::Display>(header: &str, reasons: &[T]) -> String {
        let max_width = 60;
        let mut string = format!("{header}:");
        let mut num_left = reasons.len();
        for reason in reasons.iter().reservoir_sample(5) {
            if string.len() < max_width {
                string += &format!(" {reason}");
                num_left -= 1;
            } else {
                string += &format!(" … + {num_left} more");
                break;
            }
        }
        string
    }

    if !crates.is_empty() {
        format_long_list("dependencies", &crates)
    } else if !path_matched_rules.is_empty() {
        format_long_list("rule for", &path_matched_rules)
    } else if !symbol_matched_rules.is_empty() {
        let symbol_matched_rules = symbol_matched_rules
            .into_iter()
            .map(|s| &s.demangled)
            .collect_vec();
        format_long_list("rule for", &symbol_matched_rules)
    } else if !unmatched_paths.is_empty() {
        format_long_list("unknown paths", &unmatched_paths)
    } else if !unmatched_symbols.is_empty() {
        let unmatched_symbols = unmatched_symbols
            .into_iter()
            .map(|s| &s.demangled)
            .collect_vec();
        format_long_list("unknown symbols", &unmatched_symbols)
    } else if !source_parse_errors.is_empty() {
        format_long_list("source parse error", &source_parse_errors)
    } else if !source_code_locations.is_empty() {
        format_long_list("source code", &source_code_locations)
    } else {
        unreachable!()
    }
}
