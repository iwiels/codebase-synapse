use anyhow::Result;
use rusqlite::Connection;

use crate::db::{self, schema::Node};
use crate::parser::extractors::ExtractionResult;

pub struct GraphBuilder<'a> {
    conn: &'a Connection,
    project_id: i64,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(conn: &'a Connection, project_id: i64) -> Self {
        Self { conn, project_id }
    }

    pub fn add_file_nodes(
        &mut self,
        file_path: &str,
        result: ExtractionResult,
    ) -> Result<Vec<i64>> {
        let mut node_ids = Vec::new();

        let file_node_id = db::queries::insert_node(
            self.conn,
            self.project_id,
            &Node {
                id: 0,
                project_id: self.project_id,
                file_path: file_path.to_string(),
                kind: "file".to_string(),
                name: Some(
                    file_path
                        .replace('\\', "/")
                        .rsplit('/')
                        .next()
                        .unwrap_or(file_path)
                        .to_string(),
                ),
                qualified_name: Some(file_path.to_string()),
                signature: None,
                doc_comment: None,
                start_line: 1,
                end_line: result.entities.iter().map(|e| e.end_line as i64).max().unwrap_or(1),
                complexity: None,
                is_exported: false,
                content_hash: None,
                source: None,
                metadata: None,
                created_at: String::new(),
                updated_at: String::new(),
            },
        )?;
        node_ids.push(file_node_id);

        for entity in &result.entities {
            let node_id = db::queries::insert_node(
                self.conn,
                self.project_id,
                &Node {
                    id: 0,
                    project_id: self.project_id,
                    file_path: file_path.to_string(),
                    kind: entity.kind.to_string(),
                    name: entity.name.clone(),
                    qualified_name: entity.qualified_name.clone(),
                    signature: entity.signature.clone(),
                    doc_comment: entity.doc_comment.clone(),
                    start_line: entity.start_line as i64,
                    end_line: entity.end_line as i64,
                    complexity: entity.complexity,
                    is_exported: entity.is_exported,
                    content_hash: None,
                    source: Some(entity.source.clone()),
                    metadata: entity.metadata.clone(),
                    created_at: String::new(),
                    updated_at: String::new(),
                },
            )?;

            db::queries::insert_edge(
                self.conn,
                self.project_id,
                file_node_id,
                node_id,
                "contains",
                None,
            )?;

            node_ids.push(node_id);
        }

        Ok(node_ids)
    }

    pub fn update_counts(&mut self) -> Result<()> {
        db::queries::update_project_counts(self.conn, self.project_id)?;
        Ok(())
    }
}
