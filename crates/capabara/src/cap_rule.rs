use std::collections::BTreeSet;

use crate::capability::Capability;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Match {
    /// Any rust path or link symbol that exactly matches this
    Exact(String),

    /// Any rust path or link symbol that start with this
    StartsWith(String),
}

pub struct Rule {
    /// If the symbol matches this…
    pub matches: BTreeSet<Match>,

    /// …then it is known to have these, and only these, capabitites
    pub caps: BTreeSet<Capability>,
}

pub struct Rules {
    /// Most specific match wins! So if `foo::bar` matches, then `foo` is ignored.
    pub rules: Vec<Rule>,
}

impl Rules {
    /// Find the most specific matching rule for a symbol
    pub fn match_symbol(&self, symbol: &str) -> Option<&BTreeSet<Capability>> {
        let mut best_match: Option<(&Rule, usize)> = None;

        for rule in &self.rules {
            for m in &rule.matches {
                match m {
                    Match::Exact(pattern) if pattern == symbol => {
                        let specificity = pattern.len();
                        if best_match
                            .as_ref()
                            .map_or(true, |(_, prev_spec)| specificity > *prev_spec)
                        {
                            best_match = Some((rule, specificity));
                        }
                    }
                    Match::StartsWith(pattern) if symbol.starts_with(pattern) => {
                        let specificity = pattern.len();
                        if best_match
                            .as_ref()
                            .map_or(true, |(_, prev_spec)| specificity > *prev_spec)
                        {
                            best_match = Some((rule, specificity));
                        }
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(rule, _)| &rule.caps)
    }
}

pub fn default_rules() -> Rules {
    let mut rules = Vec::new();

    // Safe/math functions (no capabilities)
    rules.push(Rule {
        matches: [
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
            // Misc:
            "tlv_bootstrap", // Thread Local Variable
        ]
        .iter()
        .map(|s| Match::Exact(s.to_string()))
        .collect(),
        caps: BTreeSet::new(), // No capabilities
    });

    // Functions starting with "anon." are safe
    rules.push(Rule {
        matches: [Match::StartsWith("anon.".to_string())]
            .into_iter()
            .collect(),
        caps: BTreeSet::new(),
    });

    // Allocation functions
    rules.push(Rule {
        matches: [
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
        ]
        .iter()
        .map(|s| Match::Exact(s.to_string()))
        .collect(),
        caps: [Capability::Alloc].into_iter().collect(),
    });

    // Panic functions
    rules.push(Rule {
        matches: [
            "rust_panic",
            "rust_begin_unwind",
            "rust_drop_panic",
            "rust_start_panic",
            "rust_panic_cleanup",
            "rust_foreign_exception",
        ]
        .iter()
        .map(|s| Match::Exact(s.to_string()))
        .collect(),
        caps: [Capability::Panic].into_iter().collect(),
    });

    // Core panicking
    rules.push(Rule {
        matches: [Match::StartsWith("core::panicking".to_string())]
            .into_iter()
            .collect(),
        caps: [Capability::Panic].into_iter().collect(),
    });

    // Allocation crate
    rules.push(Rule {
        matches: [Match::StartsWith("alloc".to_string())]
            .into_iter()
            .collect(),
        caps: [Capability::Alloc].into_iter().collect(),
    });

    // Std library - safe/allowlisted paths (only panic + alloc)
    rules.push(Rule {
        matches: [
            "std::path::Path::extension",
            "std::process::abort",
            "std::process::exit",
            "std::sys::os_str",
            "std::sys::pal::unix::abort_internal",
            "std::sys::pal::unix::sync",
            "std::sys::random",
            "std::sys::sync",
            "std::sys::thread_local",
            "std::thread::local",
        ]
        .iter()
        .map(|s| Match::StartsWith(s.to_string()))
        .collect(),
        caps: [Capability::Panic, Capability::Alloc].into_iter().collect(),
    });

    // Std library - system info
    rules.push(Rule {
        matches: [
            Match::StartsWith("std::sys::backtrace".to_string()),
            Match::StartsWith("std::env".to_string()),
        ]
        .into_iter()
        .collect(),
        caps: [Capability::Panic, Capability::Alloc, Capability::Sysinfo]
            .into_iter()
            .collect(),
    });

    // Std library - threading
    rules.push(Rule {
        matches: [
            Match::StartsWith("std::sys::pal::unix::thread::Thread".to_string()),
            Match::StartsWith("std::thread".to_string()),
        ]
        .into_iter()
        .collect(),
        caps: [Capability::Panic, Capability::Alloc, Capability::Thread]
            .into_iter()
            .collect(),
    });

    // Std library - stdio
    rules.push(Rule {
        matches: [
            Match::StartsWith("std::sys::pal::unix::stdio".to_string()),
            Match::StartsWith("std::io".to_string()),
        ]
        .into_iter()
        .collect(),
        caps: [Capability::Panic, Capability::Alloc, Capability::Stdio]
            .into_iter()
            .collect(),
    });

    // Std library - file operations
    rules.push(Rule {
        matches: [
            Match::StartsWith("std::sys::pal::unix::fs".to_string()),
            Match::StartsWith("std::fs".to_string()),
            Match::StartsWith("std::path".to_string()),
        ]
        .into_iter()
        .collect(),
        caps: [Capability::Panic, Capability::Alloc, Capability::Fopen]
            .into_iter()
            .collect(),
    });

    // Std library - networking
    rules.push(Rule {
        matches: [Match::StartsWith("std::net".to_string())]
            .into_iter()
            .collect(),
        caps: [Capability::Panic, Capability::Alloc, Capability::Net]
            .into_iter()
            .collect(),
    });

    // Std library - time
    rules.push(Rule {
        matches: [Match::StartsWith("std::time".to_string())]
            .into_iter()
            .collect(),
        caps: [Capability::Panic, Capability::Alloc, Capability::Time]
            .into_iter()
            .collect(),
    });

    // Std library - safe modules (only panic + alloc)
    rules.push(Rule {
        matches: [
            Match::StartsWith("std::panic".to_string()),
            Match::StartsWith("std::hash".to_string()),
            Match::StartsWith("std::collections".to_string()),
            Match::StartsWith("std::panicking".to_string()),
            Match::StartsWith("std::sync".to_string()),
        ]
        .into_iter()
        .collect(),
        caps: [Capability::Panic, Capability::Alloc].into_iter().collect(),
    });

    // Std library - everything else gets "Any" (most permissive)
    rules.push(Rule {
        matches: [Match::StartsWith("std".to_string())].into_iter().collect(),
        caps: [Capability::Any].into_iter().collect(),
    });

    Rules { rules }
}
