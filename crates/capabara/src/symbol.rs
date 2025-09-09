use std::fmt;

use crate::demangle::demangle_symbol;

#[derive(Debug, Clone)]
pub struct Symbol {
    pub mangled: String,
    pub demangled: String,
    pub category: SymbolCategory,
}

impl Symbol {
    pub fn from_mangled(mangled: String) -> Self {
        let demangled = demangle_symbol(&mangled);
        let category = classify_symbol(&demangled, &mangled);
        Self {
            mangled,
            demangled,
            category,
        }
    }

}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolCategory {
    Crate(String),
    TraitImpl {
        trait_for: String,
        target_crate: String,
    },
    Compiler(String),
    System(SystemSymbolType),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SystemSymbolType {
    OutlinedFunctions,
    StubHelpers,
    LibraryFunctions,
    Symbols,
    Other(String),
}

impl fmt::Display for SymbolCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolCategory::Crate(name) => write!(f, "{}", name),
            SymbolCategory::TraitImpl {
                trait_for,
                target_crate,
            } => {
                write!(f, "trait_impl: {} → {}", trait_for, target_crate)
            }
            SymbolCategory::Compiler(name) => write!(f, "compiler: {}", name),
            SymbolCategory::System(sys_type) => match sys_type {
                SystemSymbolType::OutlinedFunctions => write!(f, "system: outlined functions"),
                SystemSymbolType::StubHelpers => write!(f, "system: stub helpers"),
                SystemSymbolType::LibraryFunctions => write!(f, "system: library functions"),
                SystemSymbolType::Symbols => write!(f, "system: symbols"),
                SystemSymbolType::Other(name) => write!(f, "system: {}", name),
            },
            SymbolCategory::Unknown => write!(f, "unknown"),
        }
    }
}

fn classify_symbol(demangled_symbol: &str, original_symbol: &str) -> SymbolCategory {
    // Handle Rust symbols that were successfully demangled
    if original_symbol != demangled_symbol
        && let Some(first_colon) = demangled_symbol.find("::")
    {
        let first_part = &demangled_symbol[..first_colon];

        // Handle trait implementations like _<Type as crate..trait>::method
        if first_part.starts_with("_<")
            && first_part.contains(" as ")
            && let Some(as_pos) = first_part.find(" as ")
        {
            let trait_for = &first_part[2..as_pos]; // Skip "_<"
            let remaining = &first_part[as_pos + 4..]; // Skip " as "

            // Extract target crate from the remaining part
            let target_crate = if let Some(dot_dot) = remaining.find("..") {
                &remaining[..dot_dot]
            } else if let Some(gt_pos) = remaining.find(">") {
                &remaining[..gt_pos]
            } else {
                remaining
            };

            return SymbolCategory::TraitImpl {
                trait_for: trait_for.to_string(),
                target_crate: target_crate.to_string(),
            };
        }

        // Handle compiler-generated symbols
        if first_part.starts_with("__rustc[") {
            return SymbolCategory::Compiler("rustc".to_string());
        }

        // Regular crate symbol
        return SymbolCategory::Crate(first_part.to_string());
    }

    // Handle undemangled but potentially Rust symbols
    if original_symbol.starts_with('_') && original_symbol.contains("::") {
        return SymbolCategory::Unknown; // Could be Rust but failed to demangle
    }

    // System/C symbols - classify by pattern
    let sys_type = SystemSymbolType::from_symbol(original_symbol);
    SymbolCategory::System(sys_type)
}

impl SystemSymbolType {
    fn from_symbol(symbol: &str) -> Self {
        if symbol.starts_with("_OUTLINED_FUNCTION_") {
            SystemSymbolType::OutlinedFunctions
        } else if symbol.contains("stub_helper") {
            SystemSymbolType::StubHelpers
        } else if symbol.starts_with('_')
            && (symbol.contains("printf")
                || symbol.contains("malloc")
                || symbol.contains("free")
                || symbol.contains("memcpy")
                || symbol.contains("strlen")
                || symbol.contains("strcmp")
                || symbol.contains("pthread")
                || symbol.starts_with("_lib")
                || symbol.starts_with("_LC_")
                || symbol.contains("objc_")
                || symbol.contains("dyld_"))
        {
            SystemSymbolType::LibraryFunctions
        } else if symbol.starts_with('_')
            && (symbol.contains("GLOBAL_OFFSET_TABLE")
                || symbol.contains("_data")
                || symbol.contains("_bss")
                || symbol.contains("_text")
                || symbol.starts_with("_l")
                || symbol.starts_with("_L"))
        {
            SystemSymbolType::Symbols
        } else {
            SystemSymbolType::Other(symbol.to_string())
        }
    }
}
