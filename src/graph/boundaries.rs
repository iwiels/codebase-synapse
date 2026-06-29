//! Architecture boundary enforcement via glob-pattern rules.

use anyhow::Result;
use globset::{Glob, GlobMatcher};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
pub struct BoundaryConfig {
    #[serde(default)]
    pub boundary: Vec<BoundaryRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BoundaryRule {
    pub from: String,
    pub deny: Vec<String>,
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub from_file: String,
    pub to_file: String,
    pub rule_index: usize,
    pub deny_pattern: String,
}

pub fn load_config(path: &Path) -> Result<BoundaryConfig> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

/// Check edges against boundary rules.
/// `files` — (file_path, node_id). `edges` — (src_id, dst_id).
pub fn check_boundaries(
    config: &BoundaryConfig,
    files: &[(String, i64)],
    edges: &[(i64, i64)],
) -> Result<Vec<Violation>> {
    let id_to_path: std::collections::HashMap<i64, &str> =
        files.iter().map(|(fp, id)| (*id, fp.as_str())).collect();

    struct Compiled {
        from_m: GlobMatcher,
        deny_ms: Vec<(GlobMatcher, String)>,
        allow_ms: Vec<GlobMatcher>,
    }
    let compiled: Vec<Compiled> = config
        .boundary
        .iter()
        .map(|rule| {
            Ok(Compiled {
                from_m: Glob::new(&rule.from)?.compile_matcher(),
                deny_ms: rule
                    .deny
                    .iter()
                    .map(|p| Ok((Glob::new(p)?.compile_matcher(), p.clone())))
                    .collect::<Result<Vec<_>>>()?,
                allow_ms: rule
                    .allow
                    .iter()
                    .map(|p| Ok(Glob::new(p)?.compile_matcher()))
                    .collect::<Result<Vec<_>>>()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut violations = Vec::new();
    for &(src_id, dst_id) in edges {
        let from_path = match id_to_path.get(&src_id) {
            Some(p) => p,
            None => continue,
        };
        let to_path = match id_to_path.get(&dst_id) {
            Some(p) => p,
            None => continue,
        };
        for (ri, cr) in compiled.iter().enumerate() {
            if !cr.from_m.is_match(from_path) {
                continue;
            }
            for (dm, dp) in &cr.deny_ms {
                if !dm.is_match(to_path) {
                    continue;
                }
                if cr.allow_ms.iter().any(|am| am.is_match(to_path)) {
                    continue;
                }
                violations.push(Violation {
                    from_file: from_path.to_string(),
                    to_file: to_path.to_string(),
                    rule_index: ri,
                    deny_pattern: dp.clone(),
                });
            }
        }
    }
    Ok(violations)
}

/// Generate TOML boundaries suggestion from cluster assignments.
pub fn suggest_boundaries(
    cluster_assignments: &std::collections::HashMap<i64, i64>,
    id_to_path: &std::collections::HashMap<i64, String>,
) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let mut clusters: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for (&nid, &cid) in cluster_assignments {
        if cid == 0 {
            continue;
        }
        if let Some(fp) = id_to_path.get(&nid) {
            let normalized = fp.replace('\\', "/");
            let prefix = normalized.split('/').take(2).collect::<Vec<_>>().join("/");
            clusters.entry(cid).or_default().insert(prefix);
        }
    }
    // Collect all unique src/ prefixes
    let all_src_prefixes: BTreeSet<String> = clusters
        .values()
        .flat_map(|ps| ps.iter().cloned())
        .filter(|p| p.starts_with("src/"))
        .collect();
    if all_src_prefixes.len() <= 1 {
        return "# Auto-generated boundary suggestions\n\n# Only one module found, no cross-boundary rules needed.\n".to_string();
    }
    let mut out = String::from(
        "# Auto-generated boundary suggestions\n\n[[boundary]]\nfrom = \"src/**\"\ndeny = [\n",
    );
    let deny_lines: Vec<String> = all_src_prefixes
        .iter()
        .map(|p| format!("  \"{}/**\"", p))
        .collect();
    out.push_str(&deny_lines.join(",\n"));
    out.push_str("\n]\n\n");
    out
}
