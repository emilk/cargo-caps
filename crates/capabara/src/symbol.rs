use std::fmt;

use anyhow::bail;

use crate::{demangle::demangle_symbol, print::PrintOptions, rust_path::RustPath};

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
            SymbolScope::Unknown => write!(f, "unknown"),
            SymbolScope::Compilation => write!(f, "local"),
            SymbolScope::Linkage => write!(f, "static"),
            SymbolScope::Dynamic => write!(f, "dynamic"),
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

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FunctionOrPath {
    Function(String),
    RustPath(RustPath),
}

impl fmt::Display for FunctionOrPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FunctionOrPath::Function(name) => write!(f, "{name}"),
            FunctionOrPath::RustPath(path) => write!(f, "{path}"),
        }
    }
}

impl FunctionOrPath {
    pub fn from_demangled(demangled: &str) -> Vec<Self> {
        if demangled.starts_with("__rustc[") {
            // Example: '__rustc[5224e6b81cd82a8f]::__rust_alloc'
            // Get part after `]::`:
            if let Some(end_bracket) = demangled.find("]::") {
                vec![FunctionOrPath::Function(
                    demangled[end_bracket + 3..].to_owned(),
                )]
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
            vec![FunctionOrPath::RustPath(RustPath::new(demangled))]
        } else {
            vec![FunctionOrPath::Function(demangled.to_owned())]
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeName {
    /// `std::collection::Vec<T>`
    RustPath(RustPath),

    Slice(Box<TypeName>),

    /// (A, B, C)
    Tuple(Vec<TypeName>),

    /// `<type_name as trait_name>`
    TypeAsTrait {
        type_name: Box<TypeName>,
        trait_name: Box<TypeName>,
    },

    /// `<type_name as trait_name>::associated_type`
    AssosiatedPath {
        type_name: Box<TypeName>,
        trait_name: Box<TypeName>,
        associated_type: RustPath,
    },
}

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RustPath(path) => write!(f, "{path}"),
            Self::Slice(element) => write!(f, "[{element}]"),
            Self::Tuple(elements) => {
                write!(f, "(")?;
                for (i, elem) in elements.iter().enumerate() {
                    if 0 < i {
                        write!(f, ", ")?;
                    }
                    write!(f, "{elem}")?;
                }
                write!(f, ")")
            }
            Self::TypeAsTrait {
                type_name,
                trait_name,
            } => {
                write!(f, "<{type_name} as {trait_name}>")
            }
            Self::AssosiatedPath {
                type_name,
                trait_name,
                associated_type,
            } => {
                write!(f, "<{type_name} as {trait_name}>::{associated_type}")
            }
        }
    }
}

impl TypeName {
    pub fn parse(symbol: &str) -> anyhow::Result<Self> {
        if symbol.starts_with("_<") {
            return Self::parse(&symbol[1..]);
        }

        // dbg!(symbol);

        if symbol.starts_with('<') {
            let mut as_pos: Option<usize> = None;

            let mut caret_depth = 0;
            for (i, c) in symbol.bytes().enumerate() {
                if caret_depth == 1 && symbol[i..].starts_with(" as ") {
                    debug_assert!(as_pos.is_none());
                    as_pos = Some(i);
                }

                match c {
                    b'<' => caret_depth += 1,
                    b'>' => caret_depth -= 1,
                    _ => {}
                }

                if caret_depth == 0 {
                    if let Some(as_pos) = as_pos {
                        let type_name = &symbol[1..as_pos];
                        let trait_name = &symbol[as_pos + 4..i];

                        if symbol[i..].starts_with(">::") {
                            // <Type as Trait>::Name
                            let associated_type = &symbol[i + 3..];
                            // dbg!(&type_name, &trait_name, &associated_type);
                            return Ok(Self::AssosiatedPath {
                                type_name: Box::new(TypeName::parse(type_name)?),
                                trait_name: Box::new(TypeName::parse(trait_name)?),
                                associated_type: RustPath::new(associated_type),
                            });
                        } else {
                            // dbg!(&type_name, &trait_name);
                            return Ok(Self::TypeAsTrait {
                                type_name: Box::new(TypeName::parse(type_name)?),
                                trait_name: Box::new(TypeName::parse(trait_name)?),
                            });
                        }
                    } else {
                        // Example: "<dyn core::any::Any>"
                        assert_eq!(i + 1, symbol.len());
                        return Ok(Self::RustPath(RustPath::new(strip_indirections(
                            &symbol[1..i],
                        ))));
                    }
                }
            }

            anyhow::bail!("Bad type name: {symbol:?}")
        } else if symbol.starts_with('(') {
            // Parse (a,b,c), taking care to only break on commas that are NOT within nested paranthesis:
            let mut elements = vec![];
            let mut parens_depth = 0;

            let mut last_start = 1;

            for (i, c) in symbol.bytes().enumerate() {
                match c {
                    b'(' => parens_depth += 1,
                    b')' => parens_depth -= 1,
                    b',' if parens_depth == 1 => {
                        elements.push(Self::parse(&symbol[last_start..i])?);
                        last_start = i + 1;
                    }
                    _ => {}
                }

                if parens_depth == 0 {
                    elements.push(Self::parse(&symbol[last_start..i])?);
                    debug_assert!(i + 1 == symbol.len()); // TODO
                }
            }

            Ok(Self::Tuple(elements))
        } else if symbol.starts_with('[') {
            if symbol.ends_with(']') {
                Ok(Self::Slice(Box::new(Self::parse(
                    &symbol[1..symbol.len() - 1],
                )?)))
            } else {
                bail!("Bad type name: {symbol:?}")
            }
        } else {
            Ok(Self::RustPath(RustPath::new(symbol)))
        }
    }

    fn collect_path(&self, paths: &mut Vec<RustPath>) {
        match self {
            TypeName::RustPath(path) => paths.push(RustPath::new(strip_indirections(path))),
            TypeName::Slice(element) => {
                element.collect_path(paths);
            }
            TypeName::Tuple(elements) => {
                for elem in elements {
                    elem.collect_path(paths);
                }
            }
            TypeName::TypeAsTrait {
                type_name,
                trait_name,
            } => {
                type_name.collect_path(paths);
                trait_name.collect_path(paths);
            }
            TypeName::AssosiatedPath {
                type_name,
                trait_name,
                associated_type: _, // Doesn't belong to a crate, so we do not care
            } => {
                type_name.collect_path(paths);
                trait_name.collect_path(paths);
            }
        }
    }
}

fn strip_indirections(path: &str) -> &str {
    let prefixes = ["&", "*", "mut", "const", "dyn", " "];

    for prefix in prefixes {
        if let Some(rest) = path.strip_prefix(prefix) {
            return strip_indirections(rest);
        }
    }
    path
}

// -----------------------------------

/// `<typename as traitname>::functioname`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TraitFnImpl {
    pub type_name: TypeName,

    pub function_name: String,
}

impl fmt::Display for TraitFnImpl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            type_name,
            function_name,
        } = self;
        write!(f, "{type_name}::{function_name}")
    }
}

impl TraitFnImpl {
    pub fn parse(symbol: &str) -> anyhow::Result<Self> {
        let symbol = symbol.replace("..", "::");
        // dbg!(&symbol);

        // Find last ">::`:
        if let Some(last_colon_pos) = symbol.rfind(">::") {
            let type_name = &symbol[..last_colon_pos + 1];
            let function_name = symbol[last_colon_pos + 3..].to_owned();
            // dbg!(&type_name, &function_name);

            Ok(Self {
                type_name: TypeName::parse(type_name)?,
                function_name,
            })
        } else {
            anyhow::bail!("Not a trait implementation symbol: {symbol:?}")
        }
    }

    pub fn paths(&self) -> Vec<RustPath> {
        // How do we categorize this?
        // This could be `impl ForeignTrait for LocalType`
        // or `impl LocalTrait for ForeignType`
        // or `impl LocalTrait for LocalType`.
        // The trait should always be namespaced to some crate,
        // but the type can be a built-in like `[T]` or `i32`.

        let Self {
            type_name,
            function_name: _,
        } = self;

        let mut paths = vec![];
        type_name.collect_path(&mut paths);

        paths
    }
}

#[test]
fn test_parse_trait_impl() {
    // TODO: handle recursive definitions like this one:

    let tests = vec![
        (
            "__ZN66_$LT$std..io..cursor..Cursor$LT$T$GT$$u20$as$u20$std..io..Read$GT$4read17h3955760825c0713eE",
            vec!["std::io::cursor::Cursor<T>", "std::io::Read"],
        ),
        (
            "_<dyn core..any..Any>::is::h10782f44127ca60f",
            vec!["core::any::Any::is"], // TODO
        ),
        (
            "<T as <std::OsString as core::From<&T>>::SpecToOsString>::spec_to_os_string",
            vec!["std::OsString", "core::From<&T>"],
        ),
        (
            "<std..io..cursor..Cursor<T> as std..io..Read>::read_exact",
            vec!["std::io::cursor::Cursor<T>", "std::io::Read"],
        ),
        (
            "<<alloc..btree..map..IntoIter<K,V,A> as core..Drop>..drop..DropGuard<K,V,A> as core..Drop>::drop",
            vec![
                "alloc::btree::map::IntoIter<K,V,A>",
                "core::Drop",
                "core::Drop",
            ],
        ),
        (
            "<<alloc..collections..btree..map..IntoIter<K,V,A> as core..ops..drop..Drop>..drop..DropGuard<K,V,A> as core..ops..drop..Drop>::drop",
            vec![
                "alloc::collections::btree::map::IntoIter<K,V,A>",
                "core::ops::drop::Drop",
                "core::ops::drop::Drop",
            ],
        ),
        (
            "<(A,B) as core::ops::range::RangeBounds<T>>::start_bound",
            vec!["core::ops::range::RangeBounds<T>"],
        ),
        (
            "<[core::mem::maybe_uninit::MaybeUninit<T>] as core::array::iter::iter_inner::PartialDrop>::partial_drop",
            vec![
                "core::mem::maybe_uninit::MaybeUninit<T>",
                "core::array::iter::iter_inner::PartialDrop",
            ],
        ),
    ];

    for (mangled, expected_paths) in tests {
        let demangled = demangle_symbol(mangled);
        let paths = FunctionOrPath::from_demangled(&demangled);
        let paths: Vec<_> = paths.into_iter().map(|p| p.to_string()).collect();
        assert_eq!(paths, expected_paths, "{demangled} ({mangled})");
    }
}

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
    )
}
