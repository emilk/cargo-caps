use std::ops::Deref;

/// Always normalized to `snake_case` (NEVER `kebab-case`).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CrateName(String);

impl CrateName {
    pub fn new(name: impl Into<String>) -> anyhow::Result<Self> {
        let name = name.into();
        if name.is_empty() {
            anyhow::bail!("Crate name cannot be empty");
        }

        let normalized = name.replace('-', "_");

        // Basic validation for valid Rust identifier characters
        if !normalized
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            anyhow::bail!("Invalid crate name: '{name}' contains invalid characters");
        }

        if normalized.starts_with(|c: char| c.is_ascii_digit()) {
            anyhow::bail!("Invalid crate name: '{name}' cannot start with a digit");
        }

        Ok(Self(normalized))
    }
}

impl std::fmt::Debug for CrateName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::fmt::Display for CrateName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for CrateName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for CrateName {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
