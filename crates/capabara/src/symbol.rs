use std::fmt;

use crate::demangle::demangle_symbol;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolScope {
    /// Unknown scope.
    Unknown,

    /// Symbol is visible to the compilation unit.
    Compilation,

    /// Symbol is visible to the static linkage unit.
    Linkage,

    /// Symbol is visible to dynamically linked objects.
    Dynamic,
}

impl fmt::Display for SymbolScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolScope::Unknown => write!(f, "unknown"),
            SymbolScope::Compilation => write!(f, "local"),
            SymbolScope::Linkage => write!(f, "static"),
            SymbolScope::Dynamic => write!(f, "dynamic"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    /// The symbol kind is unknown.
    Unknown,

    /// The symbol is for executable code.
    Text,

    /// The symbol is for a data object.
    Data,

    /// The symbol is for a section.
    Section,

    /// The symbol is the name of a file. It precedes symbols within that file.
    File,

    /// The symbol is for a code label.
    Label,

    /// The symbol is for a thread local storage entity.
    Tls,
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SymbolKind::Unknown => write!(f, "unknown"),
            SymbolKind::Text => write!(f, "function"),
            SymbolKind::Data => write!(f, "data"),
            SymbolKind::Section => write!(f, "section"),
            SymbolKind::File => write!(f, "file"),
            SymbolKind::Label => write!(f, "label"),
            SymbolKind::Tls => write!(f, "tls"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Symbol {
    pub mangled: String,
    pub demangled: String,
    pub category: SymbolCategory,
    pub scope: SymbolScope,
    pub kind: SymbolKind,
}

impl Symbol {
    pub fn with_metadata(mangled: String, scope: SymbolScope, kind: SymbolKind) -> Self {
        let demangled = demangle_symbol(&mangled);
        let category = classify_symbol(&demangled, &mangled);
        Self {
            mangled,
            demangled,
            category,
            scope,
            kind,
        }
    }

    pub fn format(&self, include_mangled: bool) -> String {
        let Self {
            mangled, demangled, ..
        } = self;
        if include_mangled && mangled != demangled {
            format!("{demangled} ({mangled})")
        } else {
            demangled.clone()
        }
    }

    pub fn format_with_metadata(&self, include_mangled: bool, show_metadata: bool) -> String {
        let base = self.format(include_mangled);
        if show_metadata {
            let scope_str = self.scope.to_string();
            let kind_str = self.kind.to_string();
            format!("{base} [{scope_str}/{kind_str}]")
        } else {
            base
        }
    }
}

/// `<typename as traitname>::functioname`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TraitFnImpl {
    /// Could be a built-in type!
    /// Could also start with `&` for references
    pub type_name: String,

    pub trait_name: String,
    pub function_name: String,
}

impl fmt::Display for TraitFnImpl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            type_name,
            trait_name,
            function_name,
        } = self;
        write!(f, "<{type_name} as {trait_name}>::{function_name}")
    }
}

impl TraitFnImpl {
    pub fn parse(demangled_symbol: &str) -> Result<Self, &'static str> {
        // Handle trait implementations like _<Typename as Traitname>::functionname::hash
        if let Some(first_colon) = demangled_symbol.find("::") {
            let first_part = &demangled_symbol[..first_colon];
            let remaining_after_first_colon = &demangled_symbol[first_colon + 2..];

            if first_part.starts_with("_<")
                && first_part.contains(" as ")
                && first_part.ends_with(">")
                && let Some(as_pos) = first_part.find(" as ")
            {
                let typename = &first_part[2..as_pos]; // Skip "_<"
                let traitname_part = &first_part[as_pos + 4..first_part.len() - 1]; // Skip " as " and ">"

                // Extract function name (everything before the next :: or hash)
                let function_name = if let Some(next_colon) = remaining_after_first_colon.find("::")
                {
                    &remaining_after_first_colon[..next_colon]
                } else {
                    remaining_after_first_colon
                };

                // Normalize by replacing .. with ::
                let normalized_typename = typename.replace("..", "::");
                let normalized_traitname = traitname_part.replace("..", "::");

                return Ok(TraitFnImpl {
                    type_name: normalized_typename,
                    trait_name: normalized_traitname,
                    function_name: function_name.to_string(),
                });
            }
        }

        Err("Not a trait implementation symbol")
    }

    pub fn crate_bucket(&self) -> String {
        let trait_crate = crate_of(&self.trait_name);
        let type_crate = crate_of(&self.type_name);

        match (trait_crate, type_crate) {
            (Some(trait_crate), Some(type_crate)) if trait_crate == type_crate => {
                trait_crate.to_string()
            }
            (Some(trait_crate), Some(type_crate)) => {
                format!("<{type_crate}::… as {trait_crate}::…>")
            }
            (Some(trait_crate), None) => trait_crate.to_string(),
            (None, Some(type_crate)) => type_crate.to_string(),
            (None, None) => "unknown".to_string(),
        }
    }
}

/// Return what comes before the first `::`
fn crate_of(path: &str) -> Option<&str> {
    let path = path.trim_start_matches('&'); // Ignore references
    let path = path.trim_start_matches("dyn "); // Ignore &dyn
    if let Some(colon) = path.find("::") {
        Some(&path[..colon])
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SymbolCategory {
    Crate(String),
    TraitImpl(TraitFnImpl),
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
            SymbolCategory::TraitImpl(trait_impl) => {
                write!(f, "trait_impl: {trait_impl}")
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

        // Try parsing as a trait implementation
        if let Ok(trait_impl) = TraitFnImpl::parse(demangled_symbol) {
            return SymbolCategory::TraitImpl(trait_impl);
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
