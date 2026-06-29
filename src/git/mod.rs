pub mod archaeology;
pub mod hotspot;
pub use archaeology::GitArchaeologist;
pub use hotspot::HotspotAnalyzer;

/// Classify commit intent from message using conventional commit heuristics.
pub fn classify_intent(message: &str) -> &'static str {
    let msg = message.to_lowercase();
    let first = msg.lines().next().unwrap_or("");
    if first.starts_with("feat") || first.starts_with("add ") || first.starts_with("implement") {
        "feat"
    } else if first.starts_with("fix") || first.starts_with("bug") || first.starts_with("patch") {
        "fix"
    } else if first.starts_with("refactor") || first.starts_with("rename") || first.starts_with("restructure") {
        "refactor"
    } else if first.starts_with("test") || first.starts_with("spec") {
        "test"
    } else if first.starts_with("perf") || first.starts_with("optim") || first.starts_with("speed") {
        "perf"
    } else if first.starts_with("doc") || first.starts_with("readme") {
        "docs"
    } else if first.starts_with("chore") || first.starts_with("ci") || first.starts_with("build") {
        "chore"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_classify_intent() {
        assert_eq!(classify_intent("feat: add OAuth support"), "feat");
        assert_eq!(classify_intent("fix: resolve nil pointer in auth"), "fix");
        assert_eq!(classify_intent("refactor: extract payment module"), "refactor");
        assert_eq!(classify_intent("chore: update dependencies"), "chore");
        assert_eq!(classify_intent("random commit"), "other");
    }
}
