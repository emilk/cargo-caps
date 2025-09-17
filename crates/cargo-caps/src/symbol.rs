use std::fmt;

use crate::{
    demangle::demangle_symbol, print::PrintOptions, rust_path::RustPath, rust_type::TraitFnImpl,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
            Self::Unknown => write!(f, "unknown"),
            Self::Compilation => write!(f, "local"),
            Self::Linkage => write!(f, "static"),
            Self::Dynamic => write!(f, "dynamic"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
            Self::Unknown => write!(f, "unknown"),
            Self::Text => write!(f, "function"),
            Self::Data => write!(f, "data"),
            Self::Section => write!(f, "section"),
            Self::File => write!(f, "file"),
            Self::Label => write!(f, "label"),
            Self::Tls => write!(f, "tls"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FunctionOrPath {
    Function(String),
    RustPath(RustPath),
}

impl fmt::Display for FunctionOrPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Function(name) => write!(f, "{name}"),
            Self::RustPath(path) => write!(f, "{path}"),
        }
    }
}

impl FunctionOrPath {
    pub fn from_demangled(demangled: &str) -> Vec<Self> {
        if demangled.starts_with("rustc[") {
            // Example: 'rustc[5224e6b81cd82a8f]::__rust_alloc'
            // Get part after `]::`:
            if let Some(end_bracket) = demangled.find("]::") {
                vec![Self::Function(demangled[end_bracket + 3..].to_owned())]
            } else {
                panic!("Weird symbol: {demangled:?}"); // TODO
            }
        } else if let Ok(trait_impl) = TraitFnImpl::parse(demangled) {
            trait_impl
                .paths()
                .into_iter()
                .filter(|path| path.segments().len() > 1) // Probably a built-in type or generic
                .map(FunctionOrPath::RustPath)
                .collect()
        } else if demangled.contains("::") {
            vec![Self::RustPath(RustPath::new(demangled))]
        } else {
            vec![Self::Function(demangled.to_owned())]
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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

    pub fn paths(&self) -> Vec<FunctionOrPath> {
        FunctionOrPath::from_demangled(&self.demangled)
    }
}

// -----------------------------------

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_paths() {
        let symbol = Symbol::with_metadata(
            "parking_lot::raw_rwlock::RawRwLock::lock_shared_slow".to_owned(),
            SymbolScope::Dynamic,
            SymbolKind::Data,
        );
        assert_eq!(
            symbol.paths(),
            vec![FunctionOrPath::RustPath(RustPath::new(
                "parking_lot::raw_rwlock::RawRwLock::lock_shared_slow"
            ))]
        );
    }
}
