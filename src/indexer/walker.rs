use std::path::{Path, PathBuf};

use anyhow::Result;
use ignore::WalkBuilder;

use crate::parser::language::SUPPORTED_EXTENSIONS;

pub fn walk_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    let walker = WalkBuilder::new(root)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .hidden(false)
        .build();

    for entry in walker {
        let entry = entry?;
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_supported = SUPPORTED_EXTENSIONS.contains(&ext)
            || ext == "json"
            || ext == "toml"
            || ext == "mod"
            || ext == "txt"
            || ext == "yaml"
            || ext == "yml"
            || filename == "dockerfile"
            || filename.starts_with("dockerfile.");

        if is_supported {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    Ok(files)
}
