use std::collections::{BTreeMap, BTreeSet};

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use crate::{
    CrateName, Symbol,
    cap_rule::{Rules, default_rules},
    symbol::FunctionOrPath,
};

pub type CapabilitySet = BTreeSet<Capability>;

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

    /// Anything is possible, including everything else in this enum.
    #[serde(rename = "*")]
    Any,
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
            Self::Any => write!(f, "any"),
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
            Self::Any => "⚠️ ",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DeducedCapabilities {
    /// The known capabilities of this crate
    pub own_caps: BTreeMap<Capability, Reasons>,

    /// The crates we depend on that we know the capabilities of
    pub known_crates: BTreeMap<CrateName, CapabilitySet>,

    /// We couldn't classify these symbols
    pub unknown_symbols: BTreeSet<Symbol>,

    /// We need to resolve these crates to see what their capabilities are
    pub unknown_crates: BTreeMap<CrateName, BTreeSet<Symbol>>,

    /// Rules for matching symbols to capabilities
    rules: Rules, // TODO: move out somewhere else
}

impl Default for DeducedCapabilities {
    fn default() -> Self {
        Self {
            own_caps: Default::default(),
            known_crates: Default::default(),
            unknown_symbols: Default::default(),
            unknown_crates: Default::default(),
            rules: default_rules(),
        }
    }
}

/// Why do we have this capability?
pub type Reasons = BTreeSet<Reason>;

pub type Reason = Symbol;

impl DeducedCapabilities {
    pub fn from_symbols(symbols: impl IntoIterator<Item = Symbol>) -> anyhow::Result<Self> {
        let mut slf = Self::default();
        for symbol in symbols {
            slf.add(&symbol)?;
        }
        Ok(slf)
    }

    pub fn total_capabilities(&self) -> CapabilitySet {
        let Self {
            own_caps,
            known_crates,
            unknown_symbols,
            unknown_crates,
            rules: _,
        } = self;

        let mut total = BTreeSet::default();

        for cap in own_caps.keys() {
            total.insert(*cap);
        }
        for caps in known_crates.values() {
            for &cap in caps {
                total.insert(cap);
            }
        }
        if !unknown_symbols.is_empty() || !unknown_crates.is_empty() {
            total.insert(Capability::Any);
        }

        if total.contains(&Capability::Any) {
            return std::iter::once(Capability::Any).collect();
        }

        total
    }

    /// Capability from symbol
    fn add(&mut self, symbol: &Symbol) -> anyhow::Result<()> {
        for path in symbol.paths() {
            match path {
                FunctionOrPath::Function(fun_name) => {
                    let fun_name = fun_name.trim_start_matches('_');

                    // Check rules for the symbol
                    if let Some(capabilities) = self.rules.match_symbol(fun_name) {
                        for &capability in capabilities {
                            self.own_caps
                                .entry(capability)
                                .or_default()
                                .insert(symbol.clone());
                        }
                    } else {
                        self.unknown_symbols.insert(symbol.clone());
                    }
                }

                FunctionOrPath::RustPath(rust_path) => {
                    let path_str = rust_path.to_string();
                    // Check rules for the path
                    if let Some(capabilities) = self.rules.match_symbol(&path_str) {
                        for &capability in capabilities {
                            self.own_caps
                                .entry(capability)
                                .or_default()
                                .insert(symbol.clone());
                        }
                    } else {
                        // No rule matched - assume an external crate:
                        let segments = rust_path.segments();

                        let crate_name = segments[0];
                        let crate_name = CrateName::new(crate_name)
                            .with_context(|| format!("mangled: {:?}", symbol.mangled))
                            .with_context(|| format!("demangled: {:?}", symbol.demangled))?;
                        self.unknown_crates
                            .entry(crate_name)
                            .or_default()
                            .insert(symbol.clone());
                    }
                }
            }
        }

        Ok(())
    }
}
