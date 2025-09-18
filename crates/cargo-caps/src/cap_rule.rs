use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::capability::CapabilitySet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Pattern {
    /// Any rust path or link symbol that exactly matches this
    Exact(String),

    /// Any rust path or link symbol that start with this
    StartsWith(String),
}

impl Pattern {
    pub fn parse_simple(s: &str) -> Self {
        let s = s.trim_start_matches('_');
        if let Some(stripped) = s.strip_suffix("*") {
            Self::StartsWith(stripped.to_owned())
        } else {
            Self::Exact(s.to_owned())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// If the symbol matches this…
    pub pattern: BTreeSet<Pattern>,

    /// …then it is known to have these, and only these, capabitites
    pub caps: CapabilitySet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    /// Most specific match wins! So if `foo::bar` matches, then `foo` is ignored.
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
struct SerializedRule {
    /// Capabilities for this rule
    caps: CapabilitySet,

    /// String patterns that will be converted to Match using `Match::from_str`
    patterns: BTreeSet<String>,
}

impl From<SerializedRule> for Rule {
    fn from(rule: SerializedRule) -> Self {
        Self {
            caps: rule.caps,
            pattern: rule
                .patterns
                .into_iter()
                .map(|s| Pattern::parse_simple(&s))
                .collect(),
        }
    }
}

impl Rules {
    /// Find the most specific matching rule for a symbol
    pub fn match_symbol(&self, symbol: &str) -> Option<&CapabilitySet> {
        let mut best_match: Option<(&Rule, usize)> = None;

        for rule in &self.rules {
            for m in &rule.pattern {
                match m {
                    Pattern::Exact(pattern) if pattern == symbol => {
                        let specificity = pattern.len();
                        if best_match
                            .as_ref()
                            .is_none_or(|(_, prev_spec)| specificity > *prev_spec)
                        {
                            best_match = Some((rule, specificity));
                        }
                    }
                    Pattern::StartsWith(pattern) if symbol.starts_with(pattern) => {
                        let specificity = pattern.len();
                        if best_match
                            .as_ref()
                            .is_none_or(|(_, prev_spec)| specificity > *prev_spec)
                        {
                            best_match = Some((rule, specificity));
                        }
                    }
                    _ => {}
                }
            }
        }

        best_match.map(|(rule, _)| &rule.caps)
    }
}

pub fn default_rules() -> Rules {
    static DEFAULT_RULES_EON: &str = include_str!("default_rules.eon");

    #[derive(serde::Deserialize)]
    struct DefaultRules {
        rules: Vec<SerializedRule>,
    }

    let loaded: DefaultRules =
        eon::from_str(DEFAULT_RULES_EON).expect("Failed to parse default_rules.eon");
    Rules {
        rules: loaded.rules.into_iter().map(|rule| rule.into()).collect(),
    }
}

#[test]
fn test_default_rules() {
    let rules = default_rules();
    assert_eq!(rules.match_symbol("unknown"), None);
    // TODO: more sanity checking
}
