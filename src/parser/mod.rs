pub mod language;
pub mod extractors;

use std::path::Path;
use anyhow::Result;
use tree_sitter::Parser;

use self::language::LanguageConfig;

#[derive(Debug, Clone)]
pub struct ParsedFile {
    pub file_path: String,
    pub language: String,
    pub source: String,
}

pub fn parse_file(file_path: &Path, source: &str) -> Result<Option<ParsedFile>> {
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let lang_config = LanguageConfig::from_extension(ext);

    match lang_config {
        Some(config) => {
            let mut parser = Parser::new();
            parser.set_language(&config.language())?;
            let _tree = parser.parse(source.as_bytes(), None)
                .ok_or_else(|| anyhow::anyhow!("Failed to parse {}", file_path.display()))?;
            Ok(Some(ParsedFile {
                file_path: file_path.to_string_lossy().to_string(),
                language: config.name.to_string(),
                source: source.to_string(),
            }))
        }
        None => Ok(None),
    }
}
