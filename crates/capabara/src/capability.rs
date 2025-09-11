use std::collections::{BTreeMap, BTreeSet};

use crate::{Symbol, rust_path::RustPath};

/// A capability a crate can be granted,
/// or is suspected of having.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

#[derive(Default)]
pub struct DeducedCapablities {
    /// The capabilities, and why we have them
    pub caps: BTreeMap<Capability, Reasons>,

    /// We couldn't classify these symbols
    pub unknown_symbols: BTreeSet<Symbol>,

    /// We need to resolve these crates to see what their capabilities are
    pub unknown_crates: BTreeMap<String, BTreeSet<Symbol>>,
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
    fn add(&mut self, mut symbol: Symbol) {
        if symbol.demangled.starts_with("__rustc[") {
            // Example: '__rustc[5224e6b81cd82a8f]::__rust_alloc'
            // Get part after `]::`:
            if let Some(end_bracket) = symbol.demangled.find("]::") {
                symbol.demangled = symbol.demangled[end_bracket + 3..].to_owned();
                let capbility = rustc_capability(&symbol.demangled);
                self.add_capability(symbol, capbility);
            } else {
                panic!("Weird symbol: {symbol:?}"); // TODO
            };
        } else if let Ok(trait_impl) = crate::symbol::TraitFnImpl::parse(&symbol.demangled) {
            symbol.demangled = trait_impl.to_string();
            let paths = trait_impl.paths();
            for path in paths {
                self.add_path(symbol.clone(), path);
            }
        } else if symbol.demangled.contains("::") {
            let path = RustPath::new(&symbol.demangled);
            self.add_path(symbol, path);
        } else {
            let capbility = system_capability(&symbol.demangled);
            self.add_capability(symbol, capbility);
        };
    }

    fn add_capability(&mut self, symbol: Symbol, capbility: Option<Capability>) {
        if let Some(capbility) = capbility {
            self.caps.entry(capbility).or_default().insert(symbol);
        } else {
            self.unknown_symbols.insert(symbol);
        }
    }

    fn add_path(&mut self, symbol: Symbol, path: RustPath) {
        let segments = path.segments();
        let capabilities = match segments[0] {
            "alloc" => vec![Capability::Alloc],
            "core" => {
                if segments[1] == "panicking" {
                    vec![Capability::Panic]
                } else {
                    vec![]
                }
            }
            "std" => {
                vec![Capability::Any] // TODO: more conservative
            }
            crate_name => {
                self.unknown_crates
                    .entry(crate_name.to_string())
                    .or_default()
                    .insert(symbol);
                return;
            }
        };

        for capability in capabilities {
            self.caps
                .entry(capability)
                .or_default()
                .insert(symbol.clone());
        }
    }
}

fn rustc_capability(demangled: &str) -> Option<Capability> {
    match demangled {
        "__rust_alloc" => Some(Capability::Alloc),
        _ => None, // Unknown
    }
}

fn system_capability(_demangled: &str) -> Option<Capability> {
    None
}
