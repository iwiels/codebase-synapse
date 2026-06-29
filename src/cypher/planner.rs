use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::cypher::parser::{CypherQuery, PatternElement, RelDirection, WhereExpr};

pub struct CypherPlanner<'a> {
    conn: &'a Connection,
}

impl<'a> CypherPlanner<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn execute(&self, project_id: i64, query: &CypherQuery) -> Result<Value, String> {
        let (sql, params_vec) = self.plan(project_id, query)?;
        
        let mut stmt = self.conn.prepare(&sql).map_err(|e| e.to_string())?;
        
        // Convert params_vec to references for rusqlite
        let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_vec.iter().map(|p| p.as_ref()).collect();

        // Let's inspect columns to dynamic-format results
        let column_count = stmt.column_count();
        let column_names: Vec<String> = (0..column_count)
            .map(|i| stmt.column_name(i).unwrap_or("col").to_string())
            .collect();

        let mut rows = stmt.query(params_refs.as_slice()).map_err(|e| e.to_string())?;
        let mut results = Vec::new();

        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            let mut row_map = serde_json::Map::new();
            for (i, name) in column_names.iter().enumerate().take(column_count) {
                let val: Value = match row.get::<_, rusqlite::types::Value>(i) {
                    Ok(rusqlite::types::Value::Null) => Value::Null,
                    Ok(rusqlite::types::Value::Integer(v)) => json!(v),
                    Ok(rusqlite::types::Value::Real(v)) => json!(v),
                    Ok(rusqlite::types::Value::Text(v)) => {
                        // Check if it's JSON
                        if v.trim_start().starts_with('{') || v.trim_start().starts_with('[') {
                            serde_json::from_str(&v).unwrap_or(json!(v))
                        } else {
                            json!(v)
                        }
                    }
                    Ok(rusqlite::types::Value::Blob(v)) => json!(v),
                    Err(_) => Value::Null,
                };
                row_map.insert(name.clone(), val);
            }
            results.push(Value::Object(row_map));
        }

        Ok(json!({
            "results_count": results.len(),
            "results": results
        }))
    }

    pub fn plan(&self, project_id: i64, query: &CypherQuery) -> Result<(String, Vec<Box<dyn rusqlite::types::ToSql>>), String> {
        let mut select_fields = Vec::new();
        let mut from_clauses = Vec::new();
        let mut join_clauses = Vec::new();
        let mut where_conditions = Vec::new();
        let mut sql_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        // Always query for project_id first
        where_conditions.push("nodes_1.project_id = ?".to_string());
        sql_params.push(Box::new(project_id));

        // Maps Cypher variable name to its SQLite alias/source table
        let mut alias_map: HashMap<String, String> = HashMap::new();
        let mut element_count = 0;

        // Parse MATCH pattern
        if query.match_pattern.is_empty() {
            return Err("Empty MATCH pattern".to_string());
        }

        let mut prev_alias = String::new();

        for element in &query.match_pattern {
            match element {
                PatternElement::Node(node) => {
                    element_count += 1;
                    let alias = node.variable.clone().unwrap_or_else(|| format!("n{}", element_count));
                    let tbl_alias = format!("nodes_{}", element_count);
                    alias_map.insert(alias.clone(), tbl_alias.clone());

                    if from_clauses.is_empty() {
                        from_clauses.push(format!("nodes {}", tbl_alias));
                    }

                    if let Some(lbl) = &node.label {
                        where_conditions.push(format!("{}.kind = ?", tbl_alias));
                        sql_params.push(Box::new(lbl.to_lowercase()));
                    }

                    prev_alias = alias;
                }
                PatternElement::Rel(rel, node) => {
                    element_count += 1;
                    let rel_alias = format!("edges_{}", element_count);
                    let target_alias = node.variable.clone().unwrap_or_else(|| format!("n{}", element_count));
                    let target_tbl_alias = format!("nodes_{}", element_count);
                    alias_map.insert(target_alias.clone(), target_tbl_alias.clone());

                    let prev_tbl_alias = alias_map.get(&prev_alias).ok_or("Invalid query state")?;

                    // Join relationship table
                    match rel.direction {
                        RelDirection::Outbound => {
                            join_clauses.push(format!(
                                "JOIN edges {} ON {}.source_node_id = {}.id",
                                rel_alias, rel_alias, prev_tbl_alias
                            ));
                            join_clauses.push(format!(
                                "JOIN nodes {} ON {}.id = {}.target_node_id",
                                target_tbl_alias, target_tbl_alias, rel_alias
                            ));
                        }
                        RelDirection::Inbound => {
                            join_clauses.push(format!(
                                "JOIN edges {} ON {}.target_node_id = {}.id",
                                rel_alias, rel_alias, prev_tbl_alias
                            ));
                            join_clauses.push(format!(
                                "JOIN nodes {} ON {}.id = {}.source_node_id",
                                target_tbl_alias, target_tbl_alias, rel_alias
                            ));
                        }
                        RelDirection::Undirected => {
                            join_clauses.push(format!(
                                "JOIN edges {} ON ({}.source_node_id = {}.id OR {}.target_node_id = {}.id)",
                                rel_alias, rel_alias, prev_tbl_alias, rel_alias, prev_tbl_alias
                            ));
                            join_clauses.push(format!(
                                "JOIN nodes {} ON ({}.id = {}.target_node_id OR {}.id = {}.source_node_id) AND {}.id != {}.id",
                                target_tbl_alias, target_tbl_alias, rel_alias, target_tbl_alias, rel_alias, target_tbl_alias, prev_tbl_alias
                            ));
                        }
                    }

                    if let Some(r_type) = &rel.rel_type {
                        where_conditions.push(format!("{}.kind = ?", rel_alias));
                        sql_params.push(Box::new(r_type.to_lowercase()));
                    }

                    if let Some(lbl) = &node.label {
                        where_conditions.push(format!("{}.kind = ?", target_tbl_alias));
                        sql_params.push(Box::new(lbl.to_lowercase()));
                    }

                    prev_alias = target_alias;
                }
            }
        }

        // Translate WHERE clause
        if let Some(w) = &query.r#where {
            let w_sql = self.translate_where(w, &alias_map, &mut sql_params)?;
            where_conditions.push(w_sql);
        }

        // Build SELECT list based on RETURN clause
        for ret in &query.r#return {
            if ret.contains('.') {
                let parts: Vec<&str> = ret.split('.').collect();
                if parts.len() == 2 {
                    let var = parts[0];
                    let prop = parts[1];
                    let tbl_alias = alias_map.get(var).ok_or_else(|| format!("Unknown return variable '{}'", var))?;
                    // SQLite properties check - if prop is part of standard node schema:
                    let standard_props = ["id", "project_id", "file_path", "kind", "name", "qualified_name", "signature", "doc_comment", "start_line", "end_line", "complexity", "is_exported", "content_hash", "source", "metadata", "created_at", "updated_at"];
                    if standard_props.contains(&prop) {
                        select_fields.push(format!("{}.{} AS {var}_{prop}", tbl_alias, prop));
                    } else {
                        // Query inside properties_json
                        select_fields.push(format!("json_extract(COALESCE({}.metadata, '{{}}'), '$.{}') AS {var}_{prop}", tbl_alias, prop));
                    }
                }
            } else {
                // If it is just a node variable, return the node info
                let tbl_alias = alias_map.get(ret).ok_or_else(|| format!("Unknown return variable '{}'", ret))?;
                select_fields.push(format!("{}.id, {}.file_path, {}.kind, {}.name, {}.qualified_name, {}.start_line, {}.end_line, {}.complexity, {}.is_exported", tbl_alias, tbl_alias, tbl_alias, tbl_alias, tbl_alias, tbl_alias, tbl_alias, tbl_alias, tbl_alias));
            }
        }

        if select_fields.is_empty() {
            return Err("Nothing to RETURN".to_string());
        }

        let mut sql = format!(
            "SELECT {}\nFROM {}",
            select_fields.join(", "),
            from_clauses.join(", ")
        );

        if !join_clauses.is_empty() {
            sql.push('\n');
            sql.push_str(&join_clauses.join("\n"));
        }

        if !where_conditions.is_empty() {
            // Apply project_id scope to all joined tables
            let mut scoped_where = Vec::new();
            for cond in &where_conditions {
                scoped_where.push(cond.clone());
            }
            // Bind project_id to joined tables too
            for i in 1..=element_count {
                if i > 1 {
                    scoped_where.push(format!("nodes_{}.project_id = nodes_1.project_id", i));
                }
            }

            sql.push_str("\nWHERE ");
            sql.push_str(&scoped_where.join(" AND "));
        }

        if let Some(lim) = query.limit {
            sql.push_str(&format!("\nLIMIT {}", lim));
        }

        // ORDER BY clause
        if let Some(ob) = &query.order_by {
            if let Some(tbl) = alias_map.values().next() {
                let dir = if ob.descending { "DESC" } else { "ASC" };
                // Check if property is standard or metadata
                let standard_props = ["id", "project_id", "file_path", "kind", "name", "qualified_name", "signature", "doc_comment", "start_line", "end_line", "complexity", "is_exported", "content_hash", "source", "metadata", "created_at", "updated_at"];
                let parts: Vec<&str> = ob.property.split('.').collect();
                if parts.len() == 2 {
                    let prop = parts[1];
                    if standard_props.contains(&prop) {
                        sql.push_str(&format!("\nORDER BY {}.{} {}", tbl, prop, dir));
                    } else {
                        sql.push_str(&format!("\nORDER BY json_extract(COALESCE({}.metadata, '{{}}'), '$.{}') {}", tbl, prop, dir));
                    }
                }
            }
        }

        Ok((sql, sql_params))
    }

    fn translate_where(
        &self,
        expr: &WhereExpr,
        alias_map: &HashMap<String, String>,
        sql_params: &mut Vec<Box<dyn rusqlite::types::ToSql>>,
    ) -> Result<String, String> {
        match expr {
            WhereExpr::Eq(prop_access, value) => {
                let parts: Vec<&str> = prop_access.split('.').collect();
                if parts.len() != 2 {
                    return Err(format!("Invalid property access '{}'", prop_access));
                }
                let var = parts[0];
                let prop = parts[1];
                let tbl_alias = alias_map.get(var).ok_or_else(|| format!("Unknown variable in WHERE: '{}'", var))?;

                sql_params.push(Box::new(value.clone()));
                
                let standard_props = ["id", "project_id", "file_path", "kind", "name", "qualified_name", "signature", "doc_comment", "start_line", "end_line", "complexity", "is_exported", "content_hash", "source", "metadata", "created_at", "updated_at"];
                if standard_props.contains(&prop) {
                    Ok(format!("{}.{} = ?", tbl_alias, prop))
                } else {
                    Ok(format!("json_extract(COALESCE({}.metadata, '{{}}'), '$.{}') = ?", tbl_alias, prop))
                }
            }
            WhereExpr::NotExistsRel(start_var, rel_pattern) => {
                let start_tbl_alias = alias_map.get(start_var).ok_or_else(|| format!("Unknown variable in WHERE: '{}'", start_var))?;
                
                // NOT EXISTS (SELECT 1 FROM edges e_sub WHERE ... AND e_sub.project_id = nodes_1.project_id)
                let sub_rel_tbl = "e_sub";
                
                let direction_cond = match rel_pattern.direction {
                    RelDirection::Outbound => {
                        format!("{}.source_node_id = {}.id", sub_rel_tbl, start_tbl_alias)
                    }
                    RelDirection::Inbound => {
                        format!("{}.target_node_id = {}.id", sub_rel_tbl, start_tbl_alias)
                    }
                    RelDirection::Undirected => {
                        format!("({}.source_node_id = {}.id OR {}.target_node_id = {}.id)", sub_rel_tbl, start_tbl_alias, sub_rel_tbl, start_tbl_alias)
                    }
                };

                let mut type_cond = String::new();
                if let Some(r_type) = &rel_pattern.rel_type {
                    type_cond = format!(" AND {}.kind = ?", sub_rel_tbl);
                    sql_params.push(Box::new(r_type.to_lowercase()));
                }

                Ok(format!(
                    "NOT EXISTS (SELECT 1 FROM edges {} WHERE {}{} AND {}.project_id = {}.project_id)",
                    sub_rel_tbl, direction_cond, type_cond, sub_rel_tbl, start_tbl_alias
                ))
            }
            WhereExpr::And(left, right) => {
                let l_sql = self.translate_where(left, alias_map, sql_params)?;
                let r_sql = self.translate_where(right, alias_map, sql_params)?;
                Ok(format!("({} AND {})", l_sql, r_sql))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cypher::parser::parse_cypher;

    fn setup_mock_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // Create nodes and edges tables
        conn.execute(
            "CREATE TABLE nodes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                file_path TEXT NOT NULL,
                kind TEXT NOT NULL,
                name TEXT,
                qualified_name TEXT UNIQUE,
                signature TEXT,
                doc_comment TEXT,
                start_line INTEGER,
                end_line INTEGER,
                complexity INTEGER,
                is_exported INTEGER,
                content_hash TEXT,
                source TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                updated_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "CREATE TABLE edges (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id INTEGER NOT NULL,
                source_node_id INTEGER NOT NULL,
                target_node_id INTEGER NOT NULL,
                kind TEXT NOT NULL,
                metadata TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .unwrap();

        // Insert mock data
        conn.execute(
            "INSERT INTO nodes (project_id, file_path, kind, name, qualified_name, start_line, end_line, is_exported)
             VALUES (1, 'main.rs', 'function', 'main', 'main', 1, 10, 1)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO nodes (project_id, file_path, kind, name, qualified_name, start_line, end_line, is_exported)
             VALUES (1, 'helper.rs', 'function', 'compute', 'helper::compute', 5, 20, 1)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO nodes (project_id, file_path, kind, name, qualified_name, start_line, end_line, is_exported)
             VALUES (1, 'helper.rs', 'function', 'unused_func', 'helper::unused_func', 22, 30, 0)",
            [],
        )
        .unwrap();

        // main calls compute
        conn.execute(
            "INSERT INTO edges (project_id, source_node_id, target_node_id, kind)
             VALUES (1, 1, 2, 'calls')",
            [],
        )
        .unwrap();

        conn
    }

    #[test]
    fn test_planner_simple_match() {
        let conn = setup_mock_db();
        let planner = CypherPlanner::new(&conn);
        let q = parse_cypher("MATCH (n:Function) WHERE n.name = 'main' RETURN n.name").unwrap();
        let res = planner.execute(1, &q).unwrap();
        assert_eq!(res["results_count"], 1);
        assert_eq!(res["results"][0]["n_name"], "main");
    }

    #[test]
    fn test_planner_calls() {
        let conn = setup_mock_db();
        let planner = CypherPlanner::new(&conn);
        let q = parse_cypher("MATCH (f)-[:calls]->(g) RETURN f.name, g.name").unwrap();
        let res = planner.execute(1, &q).unwrap();
        assert_eq!(res["results_count"], 1);
        assert_eq!(res["results"][0]["f_name"], "main");
        assert_eq!(res["results"][0]["g_name"], "compute");
    }

    #[test]
    fn test_planner_dead_code_via_not_exists() {
        let conn = setup_mock_db();
        let planner = CypherPlanner::new(&conn);
        // Find functions that are not called by any function
        let q = parse_cypher("MATCH (f:Function) WHERE NOT EXISTS { (f)<-[:calls]-() } RETURN f.name").unwrap();
        let res = planner.execute(1, &q).unwrap();
        
        // main and unused_func should have 0 incoming calls (compute has 1 incoming call from main)
        assert_eq!(res["results_count"], 2);
        
        let names: Vec<String> = res["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["f_name"].as_str().unwrap().to_string())
            .collect();
        assert!(names.contains(&"main".to_string()));
        assert!(names.contains(&"unused_func".to_string()));
    }
}
