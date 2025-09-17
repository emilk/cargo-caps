//! Parsing and AST for Rust types and function paths

use crate::rust_path::RustPath;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeName {
    /// `std::collection::Vec<T>`
    RustPath(RustPath),

    /// e.g. `&mut `
    Prefixed {
        prefix: String,
        typ: Box<TypeName>,
    },

    Slice(Box<TypeName>),

    /// (A, B, C)
    Tuple(Vec<TypeName>),

    /// `<type_name as trait_name>`
    TypeAsTrait {
        type_name: Box<TypeName>,
        trait_name: Box<TypeName>,
    },

    /// `<type_name as trait_name>::associated_type`
    AssociatedPath {
        type_name: Box<TypeName>,
        trait_name: Box<TypeName>,
        associated_type: RustPath,
    },

    /// fn(A, B) -> C
    Fn {
        params: Vec<TypeName>,
        ret: Option<Box<TypeName>>,
    },
}

impl TypeName {
    pub fn parse(symbol: &str) -> anyhow::Result<Self> {
        if symbol.starts_with("_<") {
            return Self::parse(&symbol[1..]);
        }

        // dbg!(symbol);

        let prefixes = [
            "*",
            "&",
            "const",
            "dyn",
            "mut",
            "unsafe",
            r#"extern "C""#,
            " ",
        ];

        for prefix in prefixes {
            if let Some(rest) = symbol.strip_prefix(prefix) {
                return Ok(Self::Prefixed {
                    prefix: prefix.to_owned(),
                    typ: Box::new(Self::parse(rest)?),
                });
            }
        }

        if symbol.starts_with("fn(") {
            // Parse functiomn like `fn(A, B) -
            let (params, rem) = Self::parse_tuple(&symbol[2..])?;
            let ret = if let Some(typ) = rem.strip_prefix(" -> ") {
                Some(Box::new(Self::parse(typ)?))
            } else {
                None
            };
            Ok(Self::Fn { params, ret })
        } else if symbol.starts_with('<') {
            let mut as_pos: Option<usize> = None;

            let mut caret_depth = 0;
            for (i, c) in symbol.bytes().enumerate() {
                if caret_depth == 1 && symbol[i..].starts_with(" as ") {
                    debug_assert!(
                        as_pos.is_none(),
                        "Multiple 'as' keywords found in type name"
                    );
                    as_pos = Some(i);
                }

                match c {
                    b'<' => caret_depth += 1,
                    b'>' => {
                        if 0 < i && symbol.as_bytes()[i - 1] == b'-' {
                            // ignore '->'
                        } else {
                            caret_depth -= 1;
                        }
                    }
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
                            return Ok(Self::AssociatedPath {
                                type_name: Box::new(Self::parse(type_name)?),
                                trait_name: Box::new(Self::parse(trait_name)?),
                                associated_type: RustPath::new(associated_type),
                            });
                        } else {
                            // dbg!(&type_name, &trait_name);
                            return Ok(Self::TypeAsTrait {
                                type_name: Box::new(Self::parse(type_name)?),
                                trait_name: Box::new(Self::parse(trait_name)?),
                            });
                        }
                    } else {
                        anyhow::ensure!(
                            i + 1 == symbol.len(),
                            "Unexpected characters after closing bracket when parsing {symbol:?}"
                        );
                        // Example: "<dyn core::any::Any>"
                        return Ok(Self::RustPath(RustPath::new(strip_indirections(
                            &symbol[1..i],
                        ))));
                    }
                }
            }

            anyhow::bail!("Bad type name: {symbol:?}")
        } else if symbol.starts_with('(') {
            let (elements, rem) = Self::parse_tuple(symbol)?;
            anyhow::ensure!(rem.is_empty(), "Trailing stuff after tuple: {symbol:?}");
            Ok(Self::Tuple(elements))
        } else if symbol.starts_with('[') {
            if symbol.ends_with(']') {
                // [T] or [T; N]?
                if let Some(semi) = symbol.rfind("; ") {
                    // TODO: parse into Self::FixedSizeArray
                    Ok(Self::Slice(Box::new(Self::parse(&symbol[1..semi])?)))
                } else {
                    Ok(Self::Slice(Box::new(Self::parse(
                        &symbol[1..symbol.len() - 1],
                    )?)))
                }
            } else {
                anyhow::bail!("Bad type name: {symbol:?}")
            }
        } else {
            Ok(Self::RustPath(RustPath::new(symbol)))
        }
    }

    // parse (A, B, C), returning the remainder
    fn parse_tuple(symbol: &str) -> anyhow::Result<(Vec<Self>, &str)> {
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

                return Ok((elements, &symbol[i + 1..]));
            }
        }
        anyhow::bail!("Misaligned parenthesis in: {symbol:?}")
    }

    fn collect_path(&self, paths: &mut Vec<RustPath>) {
        match self {
            Self::RustPath(path) => paths.push(RustPath::new(strip_indirections(path))),
            Self::Prefixed { typ, .. } => {
                typ.collect_path(paths);
            }
            Self::Slice(element) => {
                element.collect_path(paths);
            }
            Self::Tuple(elements) => {
                for elem in elements {
                    elem.collect_path(paths);
                }
            }
            Self::TypeAsTrait {
                type_name,
                trait_name,
            }
            | Self::AssociatedPath {
                type_name,
                trait_name,
                associated_type: _, // Doesn't belong to a crate, so we do not care
            } => {
                type_name.collect_path(paths);
                trait_name.collect_path(paths);
            }
            Self::Fn { params, ret } => {
                for param in params {
                    param.collect_path(paths);
                }
                if let Some(ret) = ret {
                    ret.collect_path(paths);
                }
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

impl TraitFnImpl {
    pub fn parse(symbol: &str) -> anyhow::Result<Self> {
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

// TODO: do we need this?
fn strip_indirections(path: &str) -> &str {
    let prefixes = ["&", "*", "mut", "const", "dyn", " "];

    for prefix in prefixes {
        if let Some(rest) = path.strip_prefix(prefix) {
            return strip_indirections(rest);
        }
    }
    path
}

#[cfg(test)]
mod test {
    use crate::{demangle::demangle_symbol, symbol::FunctionOrPath};

    use super::*;

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
                vec!["core::any::Any"], // TODO
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
            (
                "__ZN77_$LT$$RF$$u5b$syn..attr..Attribute$u5d$$u20$as$u20$syn..attr..FilterAttrs$GT$5outer17h1d80fb5ca49672feE",
                vec!["syn::attr::Attribute", "syn::attr::FilterAttrs"],
            ),
            (
                r#"<extern "C" fn(&T,objc::runtime::Sel) -> R as objc::declare::MethodImplementation>::imp"#,
                vec!["objc::runtime::Sel", "objc::declare::MethodImplementation"],
            ),
            (
                "<[(K,V); N] as axum_core::response::into_response::IntoResponse>::into_response",
                vec!["axum_core::response::into_response::IntoResponse"],
            ),
        ];

        for (mangled, expected_paths) in tests {
            let demangled = demangle_symbol(mangled);
            let paths = FunctionOrPath::from_demangled(&demangled);
            let paths: Vec<_> = paths.into_iter().map(|p| p.to_string()).collect();
            assert_eq!(paths, expected_paths, "{demangled} ({mangled})");
        }
    }
}
