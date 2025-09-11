use std::fmt;

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

// -----------------------------------

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeName {
    /// `std::collection::Vec<T>`
    RustPath(RustPath),

    /// `<type_name as trait_name>`
    TypeAsTrait {
        type_name: Box<TypeName>,
        trait_name: RustPath,
    },

    /// `<type_name as trait_name>::associated_type`
    AssosiatedPath {
        type_name: Box<TypeName>,
        trait_name: RustPath,
        associated_type: RustPath,
    },
}

impl fmt::Display for TypeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeName::RustPath(path) => write!(f, "{path}"),
            TypeName::TypeAsTrait {
                type_name,
                trait_name,
            } => {
                write!(f, "<{type_name} as {trait_name}>")
            }
            TypeName::AssosiatedPath {
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

        if let Some(after_caret) = symbol.strip_prefix('<') {
            let mut caret_depth = 0;
            for (i, c) in after_caret.bytes().enumerate() {
                if caret_depth == 0
                    && let Some(trait_name_caret_assoc_type) = after_caret[i..].strip_prefix(" as ")
                {
                    let type_name = &after_caret[..i];
                    if let Some(pos) = trait_name_caret_assoc_type.find(">::") {
                        // <Type as Trait>::Name
                        let trait_name = &trait_name_caret_assoc_type[..pos];
                        let associated_type = &trait_name_caret_assoc_type[pos + 3..];
                        // dbg!(&type_name, &trait_name, &associated_type);
                        return Ok(Self::AssosiatedPath {
                            type_name: Box::new(TypeName::parse(type_name)?),
                            trait_name: RustPath::new(trait_name),
                            associated_type: RustPath::new(associated_type),
                        });
                    } else {
                        // <Type as Trait>
                        let trait_name = trait_name_caret_assoc_type;
                        if let Some(trait_name) = trait_name.strip_suffix('>') {
                            // dbg!(&type_name, &trait_name);
                            return Ok(Self::TypeAsTrait {
                                type_name: Box::new(TypeName::parse(type_name)?),
                                trait_name: RustPath::new(trait_name),
                            });
                        } else {
                            anyhow::bail!("Bad type name: {symbol:?}")
                        }
                    }
                }

                match c {
                    b'<' => caret_depth += 1,
                    b'>' => caret_depth -= 1,
                    _ => {}
                }
            }

            anyhow::bail!("Bad type name: {symbol:?}")
        } else {
            Ok(Self::RustPath(RustPath::new(symbol)))
        }
    }

    fn collect_path(&self, paths: &mut Vec<RustPath>) {
        fn strip_indirections(path: &str) -> &str {
            let prefixes = ["&", "mut", "const", "dyn", " "];

            for prefix in prefixes {
                if let Some(rest) = path.strip_prefix(prefix) {
                    return strip_indirections(rest);
                }
            }
            path
        }

        match self {
            TypeName::RustPath(path) => paths.push(RustPath::new(strip_indirections(path))),
            TypeName::TypeAsTrait {
                type_name,
                trait_name,
            } => {
                type_name.collect_path(paths);
                paths.push(RustPath::new(strip_indirections(trait_name)));
            }
            TypeName::AssosiatedPath {
                type_name,
                trait_name,
                associated_type: _, // Doesn't belong to a crate, so we do not care
            } => {
                type_name.collect_path(paths);
                paths.push(RustPath::new(strip_indirections(trait_name)));
            }
        }
    }
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

        // Find last ">::`:
        if let Some(last_colon_pos) = symbol.rfind(">::") {
            let type_name = &symbol[..last_colon_pos + 1];
            let function_name = symbol[last_colon_pos + 3..].to_owned();
            // dbg!(&type_name, &function_name);

            // Some function names ends with e.g. ::hdfea6b6d53cc7e8c - strip that:
            let function_name = if let Some(hash_pos) = function_name.rfind("::h") {
                function_name[..hash_pos].to_owned()
            } else {
                function_name
            };

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
    ];

    for (mangled, expected_paths) in tests {
        let demangled = demangle_symbol(mangled);
        let parsed = TraitFnImpl::parse(&demangled)
            .unwrap_or_else(|err| panic!("Failed to parse {demangled}: {err}"));
        assert_eq!(parsed.paths(), expected_paths);
    }
}
