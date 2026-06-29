use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    File,
    Module,
    Package,
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Interface,
    Trait,
    Type,
    Variable,
    Constant,
    Macro,
    Decorator,
    Test,
    Route,
    Tool,
    Documentation,
    Process,
    GitCommit,
    TestContract,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Package => "package",
            Self::Function => "function",
            Self::Method => "method",
            Self::Class => "class",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Type => "type",
            Self::Variable => "variable",
            Self::Constant => "constant",
            Self::Macro => "macro",
            Self::Decorator => "decorator",
            Self::Test => "test",
            Self::Route => "route",
            Self::Tool => "tool",
            Self::Documentation => "documentation",
            Self::Process => "process",
            Self::GitCommit => "git_commit",
            Self::TestContract => "test_contract",
        }
    }

    pub fn parse_name(s: &str) -> Option<Self> {
        match s {
            "file" => Some(Self::File),
            "module" => Some(Self::Module),
            "package" => Some(Self::Package),
            "function" => Some(Self::Function),
            "method" => Some(Self::Method),
            "class" => Some(Self::Class),
            "struct" => Some(Self::Struct),
            "enum" => Some(Self::Enum),
            "interface" => Some(Self::Interface),
            "trait" => Some(Self::Trait),
            "type" => Some(Self::Type),
            "variable" => Some(Self::Variable),
            "constant" => Some(Self::Constant),
            "macro" => Some(Self::Macro),
            "decorator" => Some(Self::Decorator),
            "test" => Some(Self::Test),
            "route" => Some(Self::Route),
            "tool" => Some(Self::Tool),
            "documentation" => Some(Self::Documentation),
            "process" => Some(Self::Process),
            "git_commit" => Some(Self::GitCommit),
            "test_contract" => Some(Self::TestContract),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Calls,
    CalledBy,
    Imports,
    ImportedBy,
    Extends,
    ExtendsBy,
    Implements,
    ImplementsBy,
    Contains,
    ContainedBy,
    MemberOf,
    HasMember,
    References,
    ReferencedBy,
    HandlesRoute,
    StepInProcess,
    Governs,
    DependsOn,
    TestOf,
    SimilarTo,
    EvolvedFrom,
    IntroducedBy,
    LastTouchedBy,
    VerifiesContract,
    CrossProject,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Calls => "calls",
            Self::CalledBy => "called_by",
            Self::Imports => "imports",
            Self::ImportedBy => "imported_by",
            Self::Extends => "extends",
            Self::ExtendsBy => "extends_by",
            Self::Implements => "implements",
            Self::ImplementsBy => "implements_by",
            Self::Contains => "contains",
            Self::ContainedBy => "contained_by",
            Self::MemberOf => "member_of",
            Self::HasMember => "has_member",
            Self::References => "references",
            Self::ReferencedBy => "referenced_by",
            Self::HandlesRoute => "handles_route",
            Self::StepInProcess => "step_in_process",
            Self::Governs => "governs",
            Self::DependsOn => "depends_on",
            Self::TestOf => "test_of",
            Self::SimilarTo => "similar_to",
            Self::EvolvedFrom => "evolved_from",
            Self::IntroducedBy => "introduced_by",
            Self::LastTouchedBy => "last_touched_by",
            Self::VerifiesContract => "verifies_contract",
            Self::CrossProject => "cross_project",
        }
    }

    pub fn parse_name(s: &str) -> Option<Self> {
        match s {
            "calls" => Some(Self::Calls),
            "called_by" => Some(Self::CalledBy),
            "imports" => Some(Self::Imports),
            "imported_by" => Some(Self::ImportedBy),
            "extends" => Some(Self::Extends),
            "extends_by" => Some(Self::ExtendsBy),
            "implements" => Some(Self::Implements),
            "implements_by" => Some(Self::ImplementsBy),
            "contains" => Some(Self::Contains),
            "contained_by" => Some(Self::ContainedBy),
            "member_of" => Some(Self::MemberOf),
            "has_member" => Some(Self::HasMember),
            "references" => Some(Self::References),
            "referenced_by" => Some(Self::ReferencedBy),
            "handles_route" => Some(Self::HandlesRoute),
            "step_in_process" => Some(Self::StepInProcess),
            "governs" => Some(Self::Governs),
            "depends_on" => Some(Self::DependsOn),
            "test_of" => Some(Self::TestOf),
            "similar_to" => Some(Self::SimilarTo),
            "evolved_from" => Some(Self::EvolvedFrom),
            "introduced_by" => Some(Self::IntroducedBy),
            "last_touched_by" => Some(Self::LastTouchedBy),
            "verifies_contract" => Some(Self::VerifiesContract),
            "cross_project" => Some(Self::CrossProject),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: i64,
    pub project_id: i64,
    pub file_path: String,
    pub kind: String,
    pub name: Option<String>,
    pub qualified_name: Option<String>,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
    pub complexity: Option<i64>,
    pub is_exported: bool,
    pub content_hash: Option<String>,
    pub source: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub id: i64,
    pub project_id: i64,
    pub source_node_id: i64,
    pub target_node_id: i64,
    pub kind: String,
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub root_path: String,
    pub indexed_at: Option<String>,
    pub node_count: i64,
    pub edge_count: i64,
    pub config: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNote {
    pub id: i64,
    pub project_id: i64,
    pub content: String,
    pub node_id: Option<i64>,
    pub kind: String,
    pub tags: Option<String>,
    pub access_count: i64,
    pub last_accessed: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitCommitRecord {
    pub id: i64,
    pub project_id: i64,
    pub hash: String,
    pub short_hash: String,
    pub message: String,
    pub author: String,
    pub timestamp: String,
    pub intent_kind: String, // feat|fix|refactor|chore|test|docs|perf|other
    pub files_changed: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotspotEntry {
    pub file_path: String,
    pub node_name: Option<String>,
    pub node_kind: String,
    pub complexity: i64,
    pub churn_count: i64,
    pub hotspot_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticChange {
    pub id: i64,
    pub project_id: i64,
    pub file_path: String,
    pub node_name: Option<String>,
    pub node_kind: String,
    pub change_summary: String,
    pub changed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub node: Node,
    pub score: f64,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Adr {
    pub id: i64,
    pub project_id: i64,
    pub title: String,
    pub status: String,
    pub context: String,
    pub decision: String,
    pub consequences: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS projects (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL UNIQUE,
            root_path TEXT NOT NULL,
            indexed_at TEXT,
            node_count INTEGER DEFAULT 0,
            edge_count INTEGER DEFAULT 0,
            config TEXT
        );

        CREATE TABLE IF NOT EXISTS nodes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            kind TEXT NOT NULL,
            name TEXT,
            qualified_name TEXT,
            signature TEXT,
            doc_comment TEXT,
            start_line INTEGER NOT NULL DEFAULT 0,
            end_line INTEGER NOT NULL DEFAULT 0,
            complexity INTEGER,
            is_exported INTEGER NOT NULL DEFAULT 0,
            content_hash TEXT,
            source TEXT,
            metadata TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_nodes_project ON nodes(project_id);
        CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
        CREATE INDEX IF NOT EXISTS idx_nodes_name ON nodes(name);
        CREATE INDEX IF NOT EXISTS idx_nodes_qualified ON nodes(qualified_name);
        CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(project_id, file_path);

        CREATE TABLE IF NOT EXISTS edges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            source_node_id INTEGER NOT NULL,
            target_node_id INTEGER NOT NULL,
            kind TEXT NOT NULL,
            metadata TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (source_node_id) REFERENCES nodes(id) ON DELETE CASCADE,
            FOREIGN KEY (target_node_id) REFERENCES nodes(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_edges_project ON edges(project_id);
        CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source_node_id);
        CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target_node_id);
        CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
        CREATE UNIQUE INDEX IF NOT EXISTS idx_edges_unique ON edges(source_node_id, target_node_id, kind);

        CREATE TABLE IF NOT EXISTS memory_notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            content TEXT NOT NULL,
            node_id INTEGER,
            kind TEXT NOT NULL DEFAULT 'note',
            tags TEXT,
            access_count INTEGER NOT NULL DEFAULT 0,
            last_accessed TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE SET NULL
        );

        CREATE INDEX IF NOT EXISTS idx_memory_project ON memory_notes(project_id);
        CREATE INDEX IF NOT EXISTS idx_memory_kind ON memory_notes(kind);

        CREATE TABLE IF NOT EXISTS file_states (
            project_id INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            mtime TEXT,
            last_indexed TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (project_id, file_path),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS embeddings (
            node_id INTEGER PRIMARY KEY,
            embedding BLOB NOT NULL,
            model TEXT NOT NULL,
            dimensions INTEGER NOT NULL,
            FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS git_commits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            hash TEXT NOT NULL,
            short_hash TEXT NOT NULL,
            message TEXT NOT NULL,
            author TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            intent_kind TEXT NOT NULL DEFAULT 'other',
            files_changed INTEGER NOT NULL DEFAULT 0,
            UNIQUE(project_id, hash),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_commits_project ON git_commits(project_id);
        CREATE INDEX IF NOT EXISTS idx_commits_timestamp ON git_commits(timestamp);

        CREATE TABLE IF NOT EXISTS commit_node_links (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            commit_hash TEXT NOT NULL,
            node_id INTEGER NOT NULL,
            change_type TEXT NOT NULL DEFAULT 'modified',
            UNIQUE(commit_hash, node_id),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
            FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_cnl_node ON commit_node_links(node_id);
        CREATE INDEX IF NOT EXISTS idx_cnl_commit ON commit_node_links(commit_hash);

        CREATE TABLE IF NOT EXISTS semantic_changes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            node_name TEXT,
            node_kind TEXT NOT NULL DEFAULT 'function',
            change_summary TEXT NOT NULL,
            changed_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_sc_project ON semantic_changes(project_id);
        CREATE INDEX IF NOT EXISTS idx_sc_changed_at ON semantic_changes(changed_at);

        CREATE TABLE IF NOT EXISTS node_access_log (
            node_id INTEGER NOT NULL,
            project_id INTEGER NOT NULL,
            access_count INTEGER NOT NULL DEFAULT 1,
            last_accessed TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (node_id, project_id),
            FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS adrs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL,
            context TEXT NOT NULL,
            decision TEXT NOT NULL,
            consequences TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS node_pagerank (
            node_id INTEGER PRIMARY KEY,
            project_id INTEGER NOT NULL,
            pagerank REAL NOT NULL DEFAULT 0.0,
            computed_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (node_id) REFERENCES nodes(id) ON DELETE CASCADE,
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_npr_project ON node_pagerank(project_id);
        CREATE INDEX IF NOT EXISTS idx_npr_rank ON node_pagerank(project_id, pagerank DESC);

        CREATE TABLE IF NOT EXISTS file_clusters (
            project_id INTEGER NOT NULL,
            file_path TEXT NOT NULL,
            cluster_id INTEGER NOT NULL,
            computed_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (project_id, file_path),
            FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_fc_project ON file_clusters(project_id);
        CREATE INDEX IF NOT EXISTS idx_fc_cluster ON file_clusters(project_id, cluster_id);
        ",
    )?;

    let existing_tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='nodes_fts'")?
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    if existing_tables.is_empty() {
        conn.execute_batch(
            "
            CREATE VIRTUAL TABLE IF NOT EXISTS nodes_fts USING fts5(
                name, qualified_name, signature, doc_comment, source,
                content='nodes',
                content_rowid='id',
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS nodes_ai AFTER INSERT ON nodes BEGIN
                INSERT INTO nodes_fts(rowid, name, qualified_name, signature, doc_comment, source)
                VALUES (new.id, new.name, new.qualified_name, new.signature, new.doc_comment, new.source);
            END;

            CREATE TRIGGER IF NOT EXISTS nodes_ad AFTER DELETE ON nodes BEGIN
                INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, signature, doc_comment, source)
                VALUES ('delete', old.id, old.name, old.qualified_name, old.signature, old.doc_comment, old.source);
            END;

            CREATE TRIGGER IF NOT EXISTS nodes_au AFTER UPDATE ON nodes BEGIN
                INSERT INTO nodes_fts(nodes_fts, rowid, name, qualified_name, signature, doc_comment, source)
                VALUES ('delete', old.id, old.name, old.qualified_name, old.signature, old.doc_comment, old.source);
                INSERT INTO nodes_fts(rowid, name, qualified_name, signature, doc_comment, source)
                VALUES (new.id, new.name, new.qualified_name, new.signature, new.doc_comment, new.source);
            END;
            ",
        )?;
    }

    Ok(())
}
