//! Intent-Aware Reasoning layer (inspired by RIPPLE, ICSE 2026:
//! "From Seed to Scope: Reasoning to Identify Change Impact Sets").
//!
//! RIPPLE's two-phase design maps onto our pipeline:
//!   * Seed-to-Scope expansion  -> change_coupling (evolutionary) + call graph (dependence) [already built]
//!   * Plan-then-Predict pruning -> this module: classify the change intent and prune
//!     false-positive impact predictions to improve PRECISION.
//!
//! This module ships with a local, dependency-free `HeuristicIntentAnalyzer`.
//! RIPPLE uses an LLM for the plan-then-predict phase; the same `IntentAnalyzer`
//! trait can be backed by an LLM later without changing the risk pipeline.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Coarse category of a code change, derived from the developer's stated intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeIntent {
    /// Docs, comments, READMEs — no runtime behavior change.
    Documentation,
    /// Log statements, tracing, debug output — observable but not logic.
    Logging,
    /// Whitespace, imports ordering, lint auto-fix — purely syntactic.
    Formatting,
    /// Restructuring without behavior change (extract, rename, move).
    Refactor,
    /// New capability / feature work.
    Feature,
    /// Correcting erroneous behavior.
    Bugfix,
    /// Auth, injection, secrets, access control.
    Security,
    /// Latency / memory / throughput improvements.
    Performance,
    /// Could not be classified.
    Unknown,
}

impl ChangeIntent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Documentation => "documentation",
            Self::Logging => "logging",
            Self::Formatting => "formatting",
            Self::Refactor => "refactor",
            Self::Feature => "feature",
            Self::Bugfix => "bugfix",
            Self::Security => "security",
            Self::Performance => "performance",
            Self::Unknown => "unknown",
        }
    }
}

/// Output of intent analysis: how the stated intent should modulate the risk model.
#[derive(Debug, Clone)]
pub struct IntentAssessment {
    pub intent: ChangeIntent,
    /// 0.0–1.0, confidence in the classification.
    pub confidence: f64,
    /// Multiplier applied to the evolutionary-coupling risk contribution.
    /// Low-impact intents (docs, logs) shrink it; high-impact intents keep it at 1.0.
    pub coupling_risk_multiplier: f64,
    /// Counterfactual strategies that become false positives for this intent and
    /// should be pruned from the report (RIPPLE's precision phase).
    pub prune_strategies: HashSet<&'static str>,
}

/// Pluggable intent analyzer. The default is heuristic/local; an LLM-backed
/// implementation can satisfy the same trait.
pub trait IntentAnalyzer: Send + Sync {
    fn analyze(&self, intent: &str) -> IntentAssessment;
}

/// Local, keyword/regex-based analyzer. No network, no model weights.
///
/// Design follows RIPPLE's observation that intent determines the *impact set*:
/// a documentation change should not trigger test-contract or coupling alerts,
/// while a security/feature change should keep the full impact set (recall).
pub struct HeuristicIntentAnalyzer;

impl IntentAnalyzer for HeuristicIntentAnalyzer {
    fn analyze(&self, intent: &str) -> IntentAssessment {
        let text = intent.to_lowercase();
        if text.trim().is_empty() {
            return IntentAssessment {
                intent: ChangeIntent::Unknown,
                confidence: 0.0,
                coupling_risk_multiplier: 1.0,
                prune_strategies: HashSet::new(),
            };
        }

        // Weighted keyword scoring. Highest score wins; ties resolve to the order below.
        let mut scores: Vec<(ChangeIntent, f64)> = Vec::new();

        let doc_kw = [
            "doc", "document", "comment", "readme", "typo", "spelling", "javadoc", "markdown",
            "wiki",
        ];
        let log_kw = [
            "log",
            "logging",
            "logger",
            "trace",
            "debug print",
            "println",
            "console.log",
            "slog",
            "tracing",
        ];
        let fmt_kw = [
            "format",
            "fmt",
            "indent",
            "whitespace",
            "lint",
            "clippy",
            "prettier",
            "style",
            "import order",
            "sort import",
        ];
        let ref_kw = [
            "refactor",
            "rename",
            "extract",
            "move",
            "restructure",
            "reorganize",
            "cleanup",
            "clean up",
            "modularize",
        ];
        let feat_kw = [
            "add",
            "implement",
            "feature",
            "support",
            "introduce",
            "new",
            "enable",
            "create",
        ];
        let bug_kw = [
            "fix",
            "bug",
            "broken",
            "crash",
            "regression",
            "incorrect",
            "wrong",
            "error",
            "panic",
            "patch",
        ];
        let sec_kw = [
            "security", "vulnerab", "auth", "inject", "xss", "csrf", "secret", "token", "password",
            "cve", "sanitiz",
        ];
        let perf_kw = [
            "performance",
            "perf",
            "optimize",
            "speed",
            "latency",
            "slow",
            "cache",
            "memory",
            "throughput",
            "alloc",
        ];

        scores.push((ChangeIntent::Documentation, count_hits(&text, &doc_kw)));
        scores.push((ChangeIntent::Logging, count_hits(&text, &log_kw)));
        scores.push((ChangeIntent::Formatting, count_hits(&text, &fmt_kw)));
        scores.push((ChangeIntent::Refactor, count_hits(&text, &ref_kw)));
        scores.push((ChangeIntent::Feature, count_hits(&text, &feat_kw)));
        scores.push((ChangeIntent::Bugfix, count_hits(&text, &bug_kw)));
        scores.push((ChangeIntent::Security, count_hits(&text, &sec_kw)));
        scores.push((ChangeIntent::Performance, count_hits(&text, &perf_kw)));

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let (best_intent, best_score) = scores[0];
        let total: f64 = scores.iter().map(|(_, s)| *s).sum();

        // Confidence: dominance of the best signal over the rest.
        let confidence = if total > 0.0 {
            (best_score / total).clamp(0.0, 1.0)
        } else {
            0.0
        };

        // Weak signal -> Unknown, keep full impact set.
        if best_score == 0.0 {
            return IntentAssessment {
                intent: ChangeIntent::Unknown,
                confidence: 0.0,
                coupling_risk_multiplier: 1.0,
                prune_strategies: HashSet::new(),
            };
        }

        let (coupling_risk_multiplier, prune_strategies) = match best_intent {
            ChangeIntent::Documentation => (
                0.15,
                strategies(&[
                    "account_change_coupling",
                    "add_test_contract",
                    "add_self_test",
                    "interface_segregation",
                    "extract_interface",
                    "isolate_call_chain",
                ]),
            ),
            ChangeIntent::Logging => (
                0.25,
                strategies(&[
                    "account_change_coupling",
                    "add_test_contract",
                    "add_self_test",
                ]),
            ),
            ChangeIntent::Formatting => (
                0.10,
                strategies(&[
                    "account_change_coupling",
                    "add_test_contract",
                    "add_self_test",
                    "interface_segregation",
                    "extract_interface",
                    "divide_and_conquer",
                    "refactor_hotspot",
                    "partition_changes",
                    "isolate_call_chain",
                ]),
            ),
            ChangeIntent::Refactor => (
                // Coupling still relevant (refactors touch shared surfaces),
                // but no new behavior to test broadly.
                0.6,
                strategies(&["add_test_contract", "add_self_test"]),
            ),
            ChangeIntent::Feature
            | ChangeIntent::Bugfix
            | ChangeIntent::Security
            | ChangeIntent::Performance
            | ChangeIntent::Unknown => (1.0, HashSet::new()),
        };

        IntentAssessment {
            intent: best_intent,
            confidence,
            coupling_risk_multiplier,
            prune_strategies,
        }
    }
}

fn strategies(list: &[&'static str]) -> HashSet<&'static str> {
    list.iter().copied().collect()
}

fn count_hits(text: &str, keywords: &[&str]) -> f64 {
    keywords.iter().filter(|kw| text.contains(*kw)).count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_intent_is_unknown() {
        let a = HeuristicIntentAnalyzer.analyze("");
        assert_eq!(a.intent, ChangeIntent::Unknown);
        assert_eq!(a.coupling_risk_multiplier, 1.0);
    }

    #[test]
    fn test_documentation_intent_prunes_coupling() {
        let a = HeuristicIntentAnalyzer.analyze("fix typo in documentation comment");
        assert_eq!(a.intent, ChangeIntent::Documentation);
        assert!(a.prune_strategies.contains("account_change_coupling"));
        assert!(a.prune_strategies.contains("add_test_contract"));
        assert!(a.coupling_risk_multiplier < 0.3);
    }

    #[test]
    fn test_security_intent_keeps_full_impact() {
        let a = HeuristicIntentAnalyzer.analyze("fix SQL injection vulnerability in auth handler");
        assert_eq!(a.intent, ChangeIntent::Security);
        assert_eq!(a.coupling_risk_multiplier, 1.0);
        assert!(a.prune_strategies.is_empty());
    }

    #[test]
    fn test_logging_intent() {
        let a = HeuristicIntentAnalyzer.analyze("add debug logging to payment flow");
        assert_eq!(a.intent, ChangeIntent::Logging);
        assert!(a.prune_strategies.contains("account_change_coupling"));
        assert_eq!(a.coupling_risk_multiplier, 0.25);
    }

    #[test]
    fn test_refactor_intent() {
        let a = HeuristicIntentAnalyzer.analyze("refactor: extract helper and rename module");
        assert_eq!(a.intent, ChangeIntent::Refactor);
        assert!(a.prune_strategies.contains("add_test_contract"));
        assert!(!a.prune_strategies.contains("account_change_coupling"));
    }
}
