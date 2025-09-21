use std::{fmt, ops::Deref};

use itertools::Itertools as _;
/// A Rust module or item path like `std::collections::Vec` or `my_crate::module::function`.
///
/// This struct encapsulates a path string and provides utilities for working with
/// Rust-style paths that use `::` as separators.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RustPath(String);

impl RustPath {
    /// Creates a new `RustPath` from a string.
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        Self(path)
    }

    pub fn from_segments(segments: impl IntoIterator<Item = String>) -> Self {
        Self::new(segments.into_iter().join("::"))
    }

    #[must_use]
    pub fn with_segment(mut self, segment: impl Into<String>) -> Self {
        self.push_segment(segment);
        self
    }

    pub fn push_segment(&mut self, segment: impl Into<String>) {
        if self.0.is_empty() {
            self.0 = segment.into();
        } else {
            self.0 = format!("{}::{}", self.0, segment.into());
        }
    }

    /// Finds all `RustPath`:s in the given string with at least two segments.
    ///
    /// Ignores matches that are prefixed by `>::` (like in `<Foo as Bar>::foo::bar`)
    pub fn find_all_with_at_least_two_segments_in(input: &str) -> Vec<Self> {
        if !input.contains("::") {
            return vec![]; // Early-out
        }

        use std::sync::LazyLock;

        // Compile the regex once at program startup
        static PATH_REGEX: LazyLock<regex::Regex> = LazyLock::new(|| {
            regex::RegexBuilder::new(
                r"
                \b
                    [A-Za-z_][A-Za-z0-9_]*      # identifier
                    (?:                         # non-capturing group
                        ::                      #   ::
                        [A-Za-z_][A-Za-z0-9_]*  #   identifier
                    )+                          # at least one ::
                \b",
            )
            .ignore_whitespace(true)
            .unicode(false)
            .build()
            .unwrap()
        });

        PATH_REGEX
            .find_iter(input)
            .filter(|m| !input[..m.start()].ends_with(">::"))
            .map(|m| Self::new(m.as_str()))
            .collect()
    }

    /// Returns the path as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Splits the path into its segments (parts separated by `:::`).
    ///
    /// Returns an empty vector if the path is empty.
    pub fn segments(&self) -> Vec<&str> {
        if self.0.is_empty() {
            Vec::new()
        } else {
            self.0.split("::").collect()
        }
    }
}

impl fmt::Display for RustPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Debug for RustPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "RustPath(\"{}\")", self.0)
    }
}

impl Deref for RustPath {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<String> for RustPath {
    fn from(path: String) -> Self {
        Self::new(path)
    }
}

impl From<&str> for RustPath {
    fn from(path: &str) -> Self {
        Self::new(path.to_owned())
    }
}

impl From<RustPath> for String {
    fn from(path: RustPath) -> Self {
        path.0
    }
}

impl PartialEq<str> for RustPath {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for RustPath {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<RustPath> for str {
    fn eq(&self, other: &RustPath) -> bool {
        self == other.0
    }
}

impl PartialEq<RustPath> for &str {
    fn eq(&self, other: &RustPath) -> bool {
        *self == other.0
    }
}

#[cfg(test)]
mod tests {
    use crate::demangle::demangle_symbol;

    use super::*;

    #[test]
    fn test_new_and_as_str() {
        let path = RustPath::new("std::collections::Vec");
        assert_eq!(path.as_str(), "std::collections::Vec");
    }

    #[test]
    fn test_segments() {
        let path = RustPath::new("std::collections::Vec");
        assert_eq!(path.segments(), vec!["std", "collections", "Vec"]);

        let empty = RustPath::new("");
        assert_eq!(empty.segments(), Vec::<&str>::new());

        let single = RustPath::new("Vec");
        assert_eq!(single.segments(), vec!["Vec"]);
    }

    #[test]
    fn test_display() {
        let path = RustPath::new("std::collections::Vec");
        assert_eq!(format!("{path}"), "std::collections::Vec");
    }

    #[test]
    fn test_debug() {
        let path = RustPath::new("std::collections::Vec");
        assert_eq!(format!("{path:?}"), "RustPath(\"std::collections::Vec\")");
    }

    #[test]
    fn test_deref() {
        let path = RustPath::new("std::collections::Vec");
        assert_eq!(&*path, "std::collections::Vec");
        assert_eq!(path.len(), 21); // String length via Deref
    }

    #[test]
    fn test_partial_eq_with_str() {
        let path = RustPath::new("std::collections::Vec");

        // RustPath == str
        assert_eq!(path, *"std::collections::Vec");
        assert_ne!(path, *"different::path");

        // RustPath == &str
        assert_eq!(path, "std::collections::Vec");
        assert_ne!(path, "different::path");

        // str == RustPath
        assert_eq!(*"std::collections::Vec", path);
        assert_ne!(*"different::path", path);

        // &str == RustPath
        assert_eq!("std::collections::Vec", path);
        assert_ne!("different::path", path);
    }

    #[test]
    fn test_path_finding() {
        let tests = vec![
            (
                "__ZN66_$LT$std..io..cursor..Cursor$LT$T$GT$$u20$as$u20$std..io..Read$GT$4read17h3955760825c0713eE",
                vec!["std::io::cursor::Cursor", "std::io::Read"],
            ),
            (
                "_<dyn core..any..Any>::is::h10782f44127ca60f",
                vec!["core::any::Any"],
            ),
            (
                "<T as <std::OsString as core::From<&T>>::SpecToOsString>::spec_to_os_string",
                vec!["std::OsString", "core::From"],
            ),
            (
                "<std..io..cursor..Cursor<T> as std..io..Read>::read_exact",
                vec!["std::io::cursor::Cursor", "std::io::Read"],
            ),
            (
                "<<alloc..btree..map..IntoIter<K,V,A> as core..Drop>..drop..DropGuard<K,V,A> as core..Drop>::drop",
                vec!["alloc::btree::map::IntoIter", "core::Drop", "core::Drop"],
            ),
            (
                "<<alloc..collections..btree..map..IntoIter<K,V,A> as core..ops..drop..Drop>..drop..DropGuard<K,V,A> as core..ops..drop..Drop>::drop",
                vec![
                    "alloc::collections::btree::map::IntoIter",
                    "core::ops::drop::Drop",
                    "core::ops::drop::Drop",
                ],
            ),
            (
                "<(A,B) as core::ops::range::RangeBounds<T>>::start_bound",
                vec!["core::ops::range::RangeBounds"],
            ),
            (
                "<[core::mem::maybe_uninit::MaybeUninit<T>] as core::array::iter::iter_inner::PartialDrop>::partial_drop",
                vec![
                    "core::mem::maybe_uninit::MaybeUninit",
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
            // let paths = FunctionOrPath::from_demangled(&demangled);
            // let paths: Vec<_> = paths.into_iter().map(|p| p.to_string()).collect();
            let paths = RustPath::find_all_with_at_least_two_segments_in(&demangled);
            assert_eq!(paths, expected_paths, "{demangled} ({mangled})");
        }
    }
}
