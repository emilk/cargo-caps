use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    Symbol,
    cap_rule::{Rules, default_rules},
    symbol::FunctionOrPath,
};

/// A capability a crate can be granted,
/// or is suspected of having.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Capability {
    /// Call [`panic!`]
    Panic,

    /// Allocate memory (`Box::new`, `Vec::new`, …)
    Alloc,

    /// Read the current time and/or date
    Time,

    /// Read environment variables, process info, …
    Sysinfo,

    /// Read and write to stdin, stdout, stderr
    Stdio,

    /// Spawn thread
    Thread,

    /// Connect over the network and/or listen for incoming network traffic
    Net,

    /// Open a file on disk for reading or writing
    Fopen,

    /// Anything is possible, including everything else in this enum.
    Any,
}

pub struct DeducedCapablities {
    /// The capabilities, and why we have them
    pub caps: BTreeMap<Capability, Reasons>,

    /// We couldn't classify these symbols
    pub unknown_symbols: BTreeSet<Symbol>,

    /// We need to resolve these crates to see what their capabilities are
    pub unknown_crates: BTreeMap<String, BTreeSet<Symbol>>,

    /// Rules for matching symbols to capabilities
    rules: Rules,
}

impl Default for DeducedCapablities {
    fn default() -> Self {
        Self {
            caps: Default::default(),
            unknown_symbols: Default::default(),
            unknown_crates: Default::default(),
            rules: default_rules(),
        }
    }
}

/// Why do we have this capability?
pub type Reasons = BTreeSet<Reason>;

pub type Reason = Symbol;

impl DeducedCapablities {
    pub fn from_symbols(symbols: impl IntoIterator<Item = Symbol>) -> Self {
        let mut slf = Self::default();
        for symbol in symbols {
            slf.add(symbol);
        }
        slf
    }

    /// Capability from symbol
    fn add(&mut self, symbol: Symbol) {
        for path in symbol.paths() {
            match path {
                FunctionOrPath::Function(fun_name) => {
                    let fun_name = fun_name.trim_start_matches('_');

                    // Check rules for the symbol
                    if let Some(capabilities) = self.rules.match_symbol(fun_name) {
                        for &capability in capabilities {
                            self.caps
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
                            self.caps
                                .entry(capability)
                                .or_default()
                                .insert(symbol.clone());
                        }
                    } else {
                        // No rule matched - assume an external crate:
                        let segments = rust_path.segments();

                        let crate_name = segments[0];
                        debug_assert!(
                            !crate_name.is_empty()
                                && crate_name
                                    .chars()
                                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                            "Weird crate name: {crate_name:?} in symbol {symbol:?}"
                        );
                        self.unknown_crates
                            .entry(crate_name.to_string())
                            .or_default()
                            .insert(symbol.clone());
                    }
                }
            }
        }
    }
}
