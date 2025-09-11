use std::fmt;
use std::ops::Deref;

/// A Rust module or item path like `std::collections::Vec` or `my_crate::module::function`.
///
/// This struct encapsulates a path string and provides utilities for working with
/// Rust-style paths that use `::` as separators.
///
/// # Examples
///
/// ```
/// use capabara::rust_path::RustPath;
///
/// let path = RustPath::new("std::collections::Vec");
/// assert_eq!(path.as_str(), "std::collections::Vec");
/// assert_eq!(path.segments(), vec!["std", "collections", "Vec"]);
/// ```
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RustPath(String);

impl RustPath {
    /// Creates a new `RustPath` from a string.
    ///
    /// # Examples
    ///
    /// ```
    /// use capabara::rust_path::RustPath;
    ///
    /// let path = RustPath::new("std::io::cursor::Cursor<T>");
    /// assert_eq!(path.as_str(), "std::io::cursor::Cursor<T>");
    /// ```
    pub fn new(path: impl Into<String>) -> Self {
        let path = path.into();
        Self(path)
    }

    /// Returns the path as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use capabara::rust_path::RustPath;
    ///
    /// let path = RustPath::new("core::mem::size_of");
    /// assert_eq!(path.as_str(), "core::mem::size_of");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Splits the path into its segments (parts separated by `:::`).
    ///
    /// Returns an empty vector if the path is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use capabara::rust_path::RustPath;
    ///
    /// let path = RustPath::new("std::collections::Vec");
    /// assert_eq!(path.segments(), vec!["std", "collections", "Vec"]);
    ///
    /// let empty = RustPath::new("");
    /// assert_eq!(empty.segments(), Vec::<&str>::new());
    /// ```
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
        assert_eq!(format!("{}", path), "std::collections::Vec");
    }

    #[test]
    fn test_debug() {
        let path = RustPath::new("std::collections::Vec");
        assert_eq!(format!("{:?}", path), "RustPath(\"std::collections::Vec\")");
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
}
