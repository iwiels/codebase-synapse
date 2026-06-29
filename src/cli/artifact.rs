use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rusqlite::Connection;

/// Export the SQLite database as a compressed artifact.
/// Uses `VACUUM INTO` to create a clean copy, then compresses with zstd.
pub fn export_graph(conn: &Connection, output_path: Option<&str>) -> Result<PathBuf> {
    let out = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("codebase-graph.zst"));

    let temp_dir = std::env::temp_dir().join("codebase-synapse-export");
    fs::create_dir_all(&temp_dir)?;
    let temp_db = temp_dir.join("export.db");

    let db_path = temp_db.display().to_string().replace('\'', "''");
    conn.execute_batch(&format!("VACUUM INTO '{}'", db_path))?;

    let data = fs::read(&temp_db)?;
    let compressed =
        zstd::encode_all(std::io::Cursor::new(data), 9).context("Failed to compress with zstd")?;

    fs::write(&out, &compressed)?;

    let _ = fs::remove_dir_all(&temp_dir);

    let size_mb = compressed.len() as f64 / 1_048_576.0;
    println!("  ✓ Export complete: {} ({:.1} MB)", out.display(), size_mb);

    Ok(out)
}

/// Import a compressed artifact as a new SQLite database.
pub fn import_graph(path: &str) -> Result<PathBuf> {
    let src = Path::new(path);
    if !src.exists() {
        anyhow::bail!("File not found: {}", path);
    }

    let temp_dir = std::env::temp_dir().join("codebase-synapse-import");
    fs::create_dir_all(&temp_dir)?;
    let temp_db = temp_dir.join("import.db");

    let compressed = fs::read(src)?;
    let decompressed = zstd::decode_all(std::io::Cursor::new(compressed))
        .context("Failed to decompress with zstd")?;

    fs::write(&temp_db, &decompressed)?;

    // Verify it's a valid SQLite database
    let conn = Connection::open(&temp_db)?;
    conn.execute_batch("SELECT COUNT(*) FROM projects")?;
    drop(conn);

    // Determine output path: strip .zst, add .db if needed
    let out_name = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("imported-graph");
    let out_path = if out_name.ends_with(".db") {
        PathBuf::from(out_name)
    } else {
        PathBuf::from(format!("{}.db", out_name))
    };

    fs::copy(&temp_db, &out_path)?;
    let _ = fs::remove_dir_all(&temp_dir);

    let size_mb = decompressed.len() as f64 / 1_048_576.0;
    println!(
        "  ✓ Import complete: {} ({:.1} MB)",
        out_path.display(),
        size_mb
    );

    Ok(out_path)
}
