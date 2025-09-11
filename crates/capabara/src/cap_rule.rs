use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::capability::Capability;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Pattern {
    /// Any rust path or link symbol that exactly matches this
    Exact(String),

    /// Any rust path or link symbol that start with this
    StartsWith(String),
}

impl Pattern {
    pub fn from_str(s: &str) -> Self {
        if s.ends_with("::*") {
            Pattern::StartsWith(s[..s.len() - 3].to_string())
        } else {
            Pattern::Exact(s.to_string())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// If the symbol matches this…
    pub pattern: BTreeSet<Pattern>,

    /// …then it is known to have these, and only these, capabitites
    pub caps: BTreeSet<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    /// Most specific match wins! So if `foo::bar` matches, then `foo` is ignored.
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
struct SerializedRule {
    /// Capabilities for this rule
    caps: BTreeSet<Capability>,

    /// String patterns that will be converted to Match using Match::from_str
    patterns: BTreeSet<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SerializedRules {
    rules: Vec<SerializedRule>,
}

impl From<SerializedRules> for Rules {
    fn from(rules: SerializedRules) -> Self {
        Self {
            rules: rules
                .rules
                .into_iter()
                .map(|rule| Rule {
                    caps: rule.caps,
                    pattern: rule
                        .patterns
                        .into_iter()
                        .map(|s| Pattern::from_str(&s))
                        .collect(),
                })
                .collect(),
        }
    }
}

impl Rules {
    /// Find the most specific matching rule for a symbol
    pub fn match_symbol(&self, symbol: &str) -> Option<&BTreeSet<Capability>> {
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
    static DEFAULT_RULES_RON: &str = include_str!("default_rules.ron");

    let string_rules: SerializedRules =
        ron::from_str(DEFAULT_RULES_RON).expect("Failed to parse default rules RON file");
    string_rules.into()
}

#[test]
fn test_default_rules() {
    let rules = default_rules();
    assert_eq!(rules.match_symbol("unknown"), None);
    // TODO: more santiy checking
}
