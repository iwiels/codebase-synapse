use crate::db::schema::HotspotEntry;
use anyhow::Result;
use rusqlite::params;

pub struct HotspotAnalyzer<'a> {
    conn: &'a rusqlite::Connection,
}

impl<'a> HotspotAnalyzer<'a> {
    pub fn new(conn: &'a rusqlite::Connection) -> Self {
        Self { conn }
    }

    fn compute_hotspot_score(complexity: i64, churn_count: i64) -> f64 {
        (complexity as f64) * (1.0 + (1.0 + churn_count as f64).ln())
    }

    /// hotspot_score = complexity * ln(1 + churn_count). Higher = more risky.
    pub fn get_hotspots(&self, project_id: i64, limit: i64) -> Result<Vec<HotspotEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                n.file_path,
                n.name,
                n.kind,
                COALESCE(n.complexity, 1) AS complexity,
                COUNT(DISTINCT cnl.commit_hash) AS churn_count
               FROM nodes n
               LEFT JOIN commit_node_links cnl ON cnl.node_id = n.id AND cnl.project_id = n.project_id
               WHERE n.project_id = ?1
                 AND n.kind IN ('function', 'method', 'class', 'struct')
               GROUP BY n.id
               HAVING complexity > 0
               ORDER BY complexity * (1 + COUNT(DISTINCT cnl.commit_hash)) DESC
               LIMIT ?2"
        )?;
        let rows = stmt.query_map(params![project_id, limit], |row| {
            Ok(HotspotEntry {
                file_path: row.get(0)?,
                node_name: row.get(1)?,
                node_kind: row.get(2)?,
                complexity: row.get(3)?,
                churn_count: row.get(4)?,
                hotspot_score: 0.0,
            })
        })?;
        let mut results: Vec<HotspotEntry> = Vec::new();
        for row in rows {
            results.push(row?);
        }
        for entry in &mut results {
            entry.hotspot_score = Self::compute_hotspot_score(entry.complexity, entry.churn_count);
        }
        Ok(results)
    }

    pub fn technical_debt_map(&self, project_id: i64) -> Result<Vec<HotspotEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                n.file_path,
                NULL AS name,
                'file' AS kind,
                SUM(COALESCE(n.complexity, 1)) AS total_complexity,
                COUNT(DISTINCT cnl.commit_hash) AS churn_count
               FROM nodes n
               LEFT JOIN commit_node_links cnl ON cnl.node_id = n.id AND cnl.project_id = n.project_id
               WHERE n.project_id = ?1
                 AND n.kind IN ('function', 'method', 'class', 'struct')
               GROUP BY n.file_path
               ORDER BY SUM(COALESCE(n.complexity, 1)) * (1 + COUNT(DISTINCT cnl.commit_hash)) DESC
               LIMIT 50"
        )?;
        let rows = stmt.query_map(params![project_id], |row| {
            Ok(HotspotEntry {
                file_path: row.get(0)?,
                node_name: row.get(1)?,
                node_kind: row.get(2)?,
                complexity: row.get(3)?,
                churn_count: row.get(4)?,
                hotspot_score: 0.0,
            })
        })?;
        let mut results: Vec<HotspotEntry> = Vec::new();
        for row in rows {
            results.push(row?);
        }
        for entry in &mut results {
            entry.hotspot_score = Self::compute_hotspot_score(entry.complexity, entry.churn_count);
        }
        Ok(results)
    }
}
