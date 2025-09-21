use cargo_metadata::camino::Utf8Path;

use crate::{Capability, CapabilitySet, CrateName};

/// What crates are allowed what capabilities?
#[derive(serde::Deserialize)]
pub struct WorkspaceConfig {
    pub rules: Vec<CrateRule>,
}

impl WorkspaceConfig {
    pub fn allow_basics() -> Self {
        Self {
            rules: vec![CrateRule {
                caps: [Capability::Alloc, Capability::Panic].into_iter().collect(),
                crates: vec![CratePattern::Any],
            }],
        }
    }

    /// What capabilities has this crate been granted?
    pub fn crate_caps(&self, crate_name: &CrateName) -> CapabilitySet {
        let mut caps = CapabilitySet::new();
        for rule in &self.rules {
            if rule.matches(crate_name) {
                caps.extend(rule.caps.iter().copied());
            }
        }
        caps
    }
}

#[derive(serde::Deserialize)]
pub struct CrateRule {
    /// What capabilities are granted?
    pub caps: CapabilitySet,

    /// What crates does the rule apply to?
    pub crates: Vec<CratePattern>,
}

impl CrateRule {
    pub fn matches(&self, crate_name: &CrateName) -> bool {
        self.crates
            .iter()
            .any(|pattern| pattern.matches(crate_name))
    }
}

impl WorkspaceConfig {
    pub fn from_path(path: &Utf8Path) -> anyhow::Result<Self> {
        let file = std::fs::read_to_string(path)
            .map_err(|err| anyhow::format_err!("Failed to load {path:?}: {err}"))?;
        eon::from_str(&file)
            .map_err(|err| anyhow::format_err!("Failed to deserialize {path:?}: {err}"))
    }
}

pub enum CratePattern {
    /// Matches any crate
    Any,

    /// Matches a specific crate
    // TODO: certain version
    Specific(CrateName),
}

impl CratePattern {
    pub fn matches(&self, crate_name: &CrateName) -> bool {
        match self {
            Self::Any => true,
            Self::Specific(name) => name == crate_name,
        }
    }
}

impl<'de> serde::Deserialize<'de> for CratePattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s == "*" {
            Ok(Self::Any)
        } else {
            let crate_name = CrateName::new(s).map_err(serde::de::Error::custom)?;
            Ok(Self::Specific(crate_name))
        }
    }
}
