use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::capability::Capability;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Match {
    /// Any rust path or link symbol that exactly matches this
    Exact(String),

    /// Any rust path or link symbol that start with this
    StartsWith(String),
}

impl Match {
    pub fn from_str(s: &str) -> Self {
        if s.ends_with("::*") {
            Match::StartsWith(s[..s.len() - 3].to_string())
        } else {
            Match::Exact(s.to_string())
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    /// If the symbol matches this…
    pub matches: BTreeSet<Match>,

    /// …then it is known to have these, and only these, capabitites
    pub caps: BTreeSet<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rules {
    /// Most specific match wins! So if `foo::bar` matches, then `foo` is ignored.
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
struct StringRule {
    /// String patterns that will be converted to Match using Match::from_str
    matches: BTreeSet<String>,
    /// Capabilities for this rule
    caps: BTreeSet<Capability>,
}

#[derive(Debug, Clone, Deserialize)]
struct StringRules {
    rules: Vec<StringRule>,
}

impl From<StringRules> for Rules {
    fn from(string_rules: StringRules) -> Self {
        Rules {
            rules: string_rules.rules.into_iter().map(|rule| Rule {
                matches: rule.matches.into_iter().map(|s| Match::from_str(&s)).collect(),
                caps: rule.caps,
            }).collect(),
        }
    }
}

impl Rules {
    /// Find the most specific matching rule for a symbol
    pub fn match_symbol(&self, symbol: &str) -> Option<&BTreeSet<Capability>> {
        let mut best_match: Option<(&Rule, usize)> = None;

        for rule in &self.rules {
            for m in &rule.matches {
                match m {
                    Match::Exact(pattern) if pattern == symbol => {
                        let specificity = pattern.len();
                        if best_match
                            .as_ref()
                            .is_none_or(|(_, prev_spec)| specificity > *prev_spec)
                        {
                            best_match = Some((rule, specificity));
                        }
                    }
                    Match::StartsWith(pattern) if symbol.starts_with(pattern) => {
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
    
    let string_rules: StringRules = ron::from_str(DEFAULT_RULES_RON).expect("Failed to parse default rules RON file");
    string_rules.into()
}
