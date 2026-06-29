use tree_sitter::Language;

#[derive(Debug, Clone)]
pub struct LanguageConfig {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub language_fn: fn() -> Language,
    pub extractor: &'static str,
}

impl LanguageConfig {
    pub fn language(&self) -> Language {
        (self.language_fn)()
    }

    pub fn from_extension(ext: &str) -> Option<&'static Self> {
        LANGUAGES.iter().find(|l| l.extensions.contains(&ext))
    }

    pub fn from_name(name: &str) -> Option<&'static Self> {
        LANGUAGES.iter().find(|l| l.name == name)
    }

    pub fn all() -> &'static [LanguageConfig] {
        LANGUAGES
    }

    pub fn is_supported(ext: &str) -> bool {
        LANGUAGES.iter().any(|l| l.extensions.contains(&ext))
    }
}

fn rust_lang() -> Language { tree_sitter_rust::LANGUAGE.into() }
fn python_lang() -> Language { tree_sitter_python::LANGUAGE.into() }
fn ts_lang() -> Language { tree_sitter_typescript::LANGUAGE_TSX.into() }
fn js_lang() -> Language { tree_sitter_javascript::LANGUAGE.into() }
fn go_lang() -> Language { tree_sitter_go::LANGUAGE.into() }
fn c_lang() -> Language { tree_sitter_c::LANGUAGE.into() }
fn cpp_lang() -> Language { tree_sitter_cpp::LANGUAGE.into() }
fn java_lang() -> Language { tree_sitter_java::LANGUAGE.into() }
fn csharp_lang() -> Language { tree_sitter_c_sharp::LANGUAGE.into() }
fn php_lang() -> Language { tree_sitter_php::LANGUAGE_PHP.into() }

pub static LANGUAGES: &[LanguageConfig] = &[
    LanguageConfig { name: "rust", extensions: &["rs"], language_fn: rust_lang, extractor: "rust" },
    LanguageConfig { name: "python", extensions: &["py"], language_fn: python_lang, extractor: "python" },
    LanguageConfig { name: "typescript", extensions: &["ts", "tsx"], language_fn: ts_lang, extractor: "typescript" },
    LanguageConfig { name: "javascript", extensions: &["js", "jsx", "mjs", "cjs"], language_fn: js_lang, extractor: "javascript" },
    LanguageConfig { name: "go", extensions: &["go"], language_fn: go_lang, extractor: "go" },
    LanguageConfig { name: "c", extensions: &["c", "h"], language_fn: c_lang, extractor: "c" },
    LanguageConfig { name: "cpp", extensions: &["cpp", "cc", "cxx", "hpp", "h"], language_fn: cpp_lang, extractor: "cpp" },
    LanguageConfig { name: "java", extensions: &["java"], language_fn: java_lang, extractor: "java" },
    LanguageConfig { name: "csharp", extensions: &["cs"], language_fn: csharp_lang, extractor: "csharp" },
    LanguageConfig { name: "php", extensions: &["php", "phtml"], language_fn: php_lang, extractor: "php" },
];

pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "c", "h",
    "cpp", "cc", "cxx", "hpp", "java", "cs", "php", "phtml",
    "rb", "sh", "bash", "kt", "kts", "html", "css", "md", "txt", "json", "toml", "yaml", "yml"
];
