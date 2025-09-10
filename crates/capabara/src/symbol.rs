use std::fmt;

use crate::demangle::demangle_symbol;
use crate::print::PrintOptions;

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

    /// The symbol is for a data object,
    /// e.g. string literals.
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
    pub scope: SymbolScope,
    pub kind: SymbolKind,
}

impl Symbol {
    pub fn with_metadata(mangled: String, scope: SymbolScope, kind: SymbolKind) -> Self {
        let demangled = demangle_symbol(&mangled);
        Self {
            mangled,
            demangled,
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

    pub fn format_with_metadata(&self, options: &PrintOptions) -> String {
        let base = self.format(options.include_mangled);
        if options.show_metadata {
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

    pub fn trait_crate(&self) -> Option<&str> {
        crate_of(&self.trait_name)
    }

    pub fn type_crate(&self) -> Option<&str> {
        crate_of(&self.type_name)
    }

    pub fn crates(&self) -> Vec<&str> {
        let mut crates = Vec::new();
        if let Some(trait_crate) = self.trait_crate() {
            crates.push(trait_crate);
        }
        if let Some(type_crate) = self.type_crate()
            && Some(type_crate) != self.trait_crate()
        {
            crates.push(type_crate);
        }
        crates
    }

    pub fn crate_bucket(&self) -> String {
        match (self.trait_crate(), self.type_crate()) {
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
