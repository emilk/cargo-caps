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
                if let Some(capbilities) = c_capabilities(&symbol.demangled) {
                    for capbility in capbilities {
                        self.add_capability(symbol.clone(), Some(capbility));
                    }
                } else {
                    self.unknown_symbols.insert(symbol);
                }
            } else {
                panic!("Weird symbol: {symbol:?}"); // TODO
            };
        } else if let Ok(trait_impl) = crate::symbol::TraitFnImpl::parse(&symbol.demangled) {
            symbol.demangled = trait_impl.to_string();
            let paths = trait_impl.paths();
            for path in paths {
                if path.segments().len() <= 1 {
                    // Probably a built-in type like `*const T`
                } else {
                    self.add_path(symbol.clone(), path);
                }
            }
        } else if symbol.demangled.contains("::") {
            let path = RustPath::new(&symbol.demangled);
            self.add_path(symbol, path);
        } else {
            //
            if let Some(capbilities) = c_capabilities(&symbol.demangled) {
                for capbility in capbilities {
                    self.add_capability(symbol.clone(), Some(capbility));
                }
            } else {
                self.unknown_symbols.insert(symbol);
            }
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
                // NOTE: anything in std can panic and allocate
                let mut caps = vec![Capability::Panic, Capability::Alloc];

                let allow_listed = [
                    "std::path::Path::extension",
                    "std::process::abort", // TODO: condier making this a capability
                    "std::process::exit",  // TODO: condier making this a capability
                    "std::sys::os_str",
                    "std::sys::pal::unix::abort_internal", // TODO: condier making this a capability
                    "std::sys::pal::unix::sync",
                    "std::sys::random", // TODO: consider making this a capability
                    "std::sys::sync",
                    "std::sys::thread_local",
                    "std::thread::local",
                ];

                let env_prefixes = ["std::sys::backtrace"];

                if allow_listed.iter().any(|prefix| path.starts_with(prefix)) {
                    // ok
                } else if env_prefixes.iter().any(|prefix| path.starts_with(prefix)) {
                    caps.push(Capability::Sysinfo);
                } else if path.starts_with("std::sys::pal::unix::thread::Thread") {
                    caps.push(Capability::Thread);
                } else if path.starts_with("std::sys::pal::unix::stdio") {
                    caps.push(Capability::Stdio);
                } else if path.starts_with("std::sys::pal::unix::fs") {
                    caps.push(Capability::Fopen);
                } else {
                    match segments[1] {
                        "panic" | "hash" | "collections" | "panicking" | "sync" => {}

                        "env" => caps.push(Capability::Sysinfo),
                        "fs" => caps.push(Capability::Fopen),
                        "io" => caps.push(Capability::Stdio),
                        "net" => caps.push(Capability::Net),
                        "path" => caps.push(Capability::Fopen),
                        "thread" => caps.push(Capability::Thread),
                        "time" => caps.push(Capability::Time), // TODO Not everything in time actually reads the time
                        _ => {
                            caps.push(Capability::Any);
                        } // TODO: more conservative
                    }
                }

                caps
            }
            crate_name => {
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

fn c_capabilities(demangled: &str) -> Option<Vec<Capability>> {
    let demangled = demangled.trim_start_matches('_');

    if demangled.starts_with("anon.") {
        return Some(vec![]); // Pretty sure these are ok
    }

    let allow = [
        // Simple mem stuff:
        "memcmp",
        "memcpy",
        "memmove",
        "memset",
        // Math:
        "atan2f",
        "bzero",
        "cos",
        "exp10",
        "expf",
        "fmod",
        "fmodf",
        "hypotf",
        "log10",
        "powidf2",
        "sin",
        "sincos_stret",
        "sincosf_stret",
        // Modulus:
        "umodti3",
        // Misc
        "tlv_bootstrap", // Thread Local Variable
    ];

    let alloc = [
        "rdl_alloc",
        "rdl_alloc_zeroed",
        "rdl_dealloc",
        "rdl_realloc",
        "rg_oom",
        "rust_alloc_error_handler",
        "rust_alloc_zeroed",
        "rust_alloc",
        "rust_dealloc",
        "rust_no_alloc_shim_is_unstable",
        "rust_realloc",
    ];

    let panic = [
        "rust_panic",
        "rust_begin_unwind",
        "rust_drop_panic",
        "rust_start_panic",
        "rust_panic_cleanup",
        "rust_foreign_exception",
    ];

    if allow.contains(&demangled) {
        return Some(vec![]); // math
    }

    if alloc.contains(&demangled) {
        return Some(vec![Capability::Alloc]);
    }

    if panic.contains(&demangled) {
        return Some(vec![Capability::Panic]);
    }

    None // Unknown
}
