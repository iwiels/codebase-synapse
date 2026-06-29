use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

fn rust_lang() -> tree_sitter::Language { tree_sitter_rust::LANGUAGE.into() }
fn python_lang() -> tree_sitter::Language { tree_sitter_python::LANGUAGE.into() }
fn ts_lang() -> tree_sitter::Language { tree_sitter_typescript::LANGUAGE_TSX.into() }
fn js_lang() -> tree_sitter::Language { tree_sitter_javascript::LANGUAGE.into() }
fn go_lang() -> tree_sitter::Language { tree_sitter_go::LANGUAGE.into() }
fn c_lang() -> tree_sitter::Language { tree_sitter_c::LANGUAGE.into() }
fn cpp_lang() -> tree_sitter::Language { tree_sitter_cpp::LANGUAGE.into() }
fn java_lang() -> tree_sitter::Language { tree_sitter_java::LANGUAGE.into() }
fn csharp_lang() -> tree_sitter::Language { tree_sitter_c_sharp::LANGUAGE.into() }
fn php_lang() -> tree_sitter::Language { tree_sitter_php::LANGUAGE_PHP.into() }

#[derive(Clone)]
pub struct ExtractedEntity {
    pub kind: &'static str,
    pub name: Option<String>,
    pub qualified_name: Option<String>,
    pub signature: Option<String>,
    pub doc_comment: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub complexity: Option<i64>,
    pub is_exported: bool,
    pub source: String,
    pub metadata: Option<String>,
}

#[derive(Clone)]
pub struct ExtractedRelation {
    pub kind: &'static str,
    pub source_name: String,
    pub target_name: String,
    pub metadata: Option<String>,
}

#[derive(Clone)]
pub struct ExtractionResult {
    pub entities: Vec<ExtractedEntity>,
    pub relations: Vec<ExtractedRelation>,
}

pub trait Extractor {
    fn extract(&self, source: &str) -> ExtractionResult;
}

fn run_query(source: &str, language: fn() -> tree_sitter::Language, query_str: &str) -> Vec<(Vec<(String, String)>, String)> {
    let mut results = Vec::new();
    let mut parser = Parser::new();
    let lang = language();
    parser.set_language(&lang).ok();
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => return results,
    };
    let root = tree.root_node();
    let query = match Query::new(&lang, query_str) {
        Ok(q) => q,
        Err(_) => return results,
    };
    let mut cursor = QueryCursor::new();
    let mut qm = cursor.matches(&query, root, source.as_bytes());
    while let Some(m) = qm.next() {
        let mut captures = Vec::new();
        for c in m.captures.iter() {
            let name = query.capture_names()[c.index as usize].to_string();
            let text = c.node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
            captures.push((name, text));
        }
        let full_text = m.captures[0].node.utf8_text(source.as_bytes()).unwrap_or("").to_string();
        results.push((captures, full_text));
    }
    results
}

pub fn get_extractor(language: &str) -> Option<Box<dyn Extractor>> {
    match language {
        "rust" => Some(Box::new(RustExtractor)),
        "python" => Some(Box::new(PythonExtractor)),
        "typescript" => Some(Box::new(TsExtractor)),
        "javascript" => Some(Box::new(JsExtractor)),
        "go" => Some(Box::new(GoExtractor)),
        "c" => Some(Box::new(CExtractor)),
        "cpp" => Some(Box::new(CppExtractor)),
        "java" => Some(Box::new(JavaExtractor)),
        "csharp" => Some(Box::new(CSharpExtractor)),
        "php" => Some(Box::new(PhpExtractor)),
        _ => None,
    }
}

struct RustExtractor;
impl Extractor for RustExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        for (captures, source_text) in run_query(source, rust_lang, "(function_item name: (identifier) @name) @func") {
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity("function", name, start_line, end_line, true, source_text));
        }
        for (captures, source_text) in run_query(source, rust_lang, "(struct_item name: (type_identifier) @name) @struct") {
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity("struct", name, start_line, end_line, true, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct PythonExtractor;
impl Extractor for PythonExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        for (captures, source_text) in run_query(source, python_lang, "(function_definition name: (identifier) @name) @func") {
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity("function", name, start_line, end_line, true, source_text));
        }
        for (captures, source_text) in run_query(source, python_lang, "(class_definition name: (identifier) @name) @class") {
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity("class", name, start_line, end_line, true, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct TsExtractor;
impl Extractor for TsExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(function_declaration name: (identifier) @name) @decl
                  (class_declaration name: (type_identifier) @name) @decl
                  (interface_declaration name: (type_identifier) @name) @decl
                  (type_alias_declaration name: (type_identifier) @name) @decl
                  (enum_declaration name: (identifier) @name) @decl";
        for (captures, source_text) in run_query(source, ts_lang, q) {
            let kind = if source_text.starts_with("function") { "function" }
                else if source_text.starts_with("class") { "class" }
                else if source_text.starts_with("interface") { "interface" }
                else if source_text.starts_with("type") { "type" }
                else if source_text.starts_with("enum") { "enum" }
                else { "declaration" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity(kind, name, start_line, end_line, source_text.contains("export"), source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct JsExtractor;
impl Extractor for JsExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(function_declaration name: (identifier) @name) @decl
                  (class_declaration name: (type_identifier) @name) @decl";
        for (captures, source_text) in run_query(source, js_lang, q) {
            let kind = if source_text.starts_with("function") { "function" } else { "class" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity(kind, name, start_line, end_line, source_text.contains("export"), source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct GoExtractor;
impl Extractor for GoExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(function_declaration name: (identifier) @name) @decl
                  (method_declaration name: (field_identifier) @name) @decl
                  (type_declaration (type_spec name: (type_identifier) @name)) @decl";
        for (captures, source_text) in run_query(source, go_lang, q) {
            let kind = if source_text.starts_with("func ") && source_text.contains("func (") { "method" }
                else if source_text.starts_with("func") { "function" }
                else { "type" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            let exported = name.as_deref().is_some_and(|n| n.chars().next().is_some_and(|c| c.is_uppercase()));
            entities.push(make_entity(kind, name, start_line, end_line, exported, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct CExtractor;
impl Extractor for CExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(function_definition declarator: (function_declarator declarator: (identifier) @name)) @decl
                  (struct_specifier name: (type_identifier) @name) @decl
                  (enum_specifier name: (type_identifier) @name) @decl";
        for (captures, source_text) in run_query(source, c_lang, q) {
            let kind = if source_text.contains("struct") { "struct" }
                else if source_text.contains("enum") { "enum" }
                else { "function" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity(kind, name, start_line, end_line, true, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct CppExtractor;
impl Extractor for CppExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(function_definition declarator: (function_declarator declarator: (identifier) @name)) @decl
                  (function_definition declarator: (function_declarator declarator: (field_identifier) @name)) @decl
                  (class_specifier name: (type_identifier) @name) @decl
                  (struct_specifier name: (type_identifier) @name) @decl";
        for (captures, source_text) in run_query(source, cpp_lang, q) {
            let kind = if source_text.contains("class") { "class" }
                else if source_text.contains("struct") { "struct" }
                else { "function" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            entities.push(make_entity(kind, name, start_line, end_line, true, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct JavaExtractor;
impl Extractor for JavaExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(class_declaration name: (identifier) @name) @decl
                  (interface_declaration name: (identifier) @name) @decl
                  (method_declaration name: (identifier) @name) @decl";
        for (captures, source_text) in run_query(source, java_lang, q) {
            let kind = if source_text.contains("class ") { "class" }
                else if source_text.contains("interface ") { "interface" }
                else { "method" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            let exported = source_text.contains("public") || source_text.contains("protected");
            entities.push(make_entity(kind, name, start_line, end_line, exported, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

struct CSharpExtractor;
impl Extractor for CSharpExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(class_declaration name: (identifier) @name) @decl
                  (interface_declaration name: (identifier) @name) @decl
                  (struct_declaration name: (identifier) @name) @decl
                  (method_declaration name: (identifier) @name) @decl";
        for (captures, source_text) in run_query(source, csharp_lang, q) {
            let kind = if source_text.contains("class ") { "class" }
                else if source_text.contains("interface ") { "interface" }
                else if source_text.contains("struct ") { "struct" }
                else { "method" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            let exported = source_text.contains("public") || source_text.contains("protected") || source_text.contains("internal");
            entities.push(make_entity(kind, name, start_line, end_line, exported, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}


struct PhpExtractor;
impl Extractor for PhpExtractor {
    fn extract(&self, source: &str) -> ExtractionResult {
        let mut entities = Vec::new();
        let q = "(class_declaration name: (name) @name) @decl
                  (interface_declaration name: (name) @name) @decl
                  (function_definition name: (name) @name) @decl
                  (method_declaration name: (name) @name) @decl";
        for (captures, source_text) in run_query(source, php_lang, q) {
            let kind = if source_text.contains("class ") { "class" }
                else if source_text.contains("interface ") { "interface" }
                else if source_text.contains("function ") && source_text.contains("function") { "function" }
                else { "method" };
            let (name, start_line, end_line) = extract_position(&captures, &source_text, source);
            let exported = !source_text.contains("private") && !source_text.contains("protected");
            entities.push(make_entity(kind, name, start_line, end_line, exported, source_text));
        }
        ExtractionResult { entities, relations: vec![] }
    }
}

fn extract_position(captures: &[(String, String)], source_text: &str, full_source: &str) -> (Option<String>, usize, usize) {
    let name = captures.iter().find(|(n, _)| n == "name").map(|(_, t)| t.clone());
    let start_line = full_source[..full_source.find(source_text).unwrap_or(0)]
        .matches('\n').count() + 1;
    let end_line = start_line + source_text.matches('\n').count();
    (name, start_line, end_line)
}

fn make_entity(kind: &'static str, name: Option<String>, start_line: usize, end_line: usize, exported: bool, source: String) -> ExtractedEntity {
    ExtractedEntity {
        kind,
        name: name.clone(),
        qualified_name: name,
        signature: Some(String::new()),
        doc_comment: None,
        start_line,
        end_line,
        complexity: Some((end_line - start_line) as i64),
        is_exported: exported,
        source,
        metadata: None,
    }
}

pub fn is_test_file(file_path: &str) -> bool {
    let path = file_path.to_lowercase().replace('\\', "/");
    let filename = path.split('/').next_back().unwrap_or(&path);
    filename.starts_with("test_")
        || filename.ends_with("_test.rs")
        || filename.ends_with("_test.py")
        || filename.ends_with("_test.go")
        || filename.ends_with(".spec.ts")
        || filename.ends_with(".spec.js")
        || filename.ends_with(".test.ts")
        || filename.ends_with(".test.js")
        || path.contains("/tests/")
        || path.contains("/test/")
        || path.contains("/__tests__/")
}

pub fn extract_tested_symbols(source: &str, language: &str) -> Vec<String> {
    let mut symbols = Vec::new();
    match language {
        "rust" => {
            // Rust test functions via #[test] attribute
            for (captures, _) in run_query(source, rust_lang,
                "(function_item name: (identifier) @name) @func")
            {
                if let Some((_, name)) = captures.iter().find(|(n, _)| n == "name") {
                    if let Some(stripped) = name.strip_prefix("test_") {
                        // Strip "test_" prefix to get likely target symbol
                        symbols.push(stripped.to_string());
                        symbols.push(name.clone());
                    }
                }
            }
        }
        "python" => {
            for (captures, _) in run_query(source, python_lang,
                "(function_definition name: (identifier) @name) @func")
            {
                if let Some((_, name)) = captures.iter().find(|(n, _)| n == "name") {
                    if let Some(stripped) = name.strip_prefix("test_") {
                        symbols.push(stripped.to_string());
                        symbols.push(name.clone());
                    }
                }
            }
        }
        "go" => {
            for (captures, _) in run_query(source, go_lang,
                "(function_declaration name: (identifier) @name) @func")
            {
                if let Some((_, name)) = captures.iter().find(|(n, _)| n == "name") {
                    if let Some(stripped) = name.strip_prefix("Test") {
                        symbols.push(stripped.to_string()); // strip "Test" prefix
                        symbols.push(name.clone());
                    }
                }
            }
        }
        _ => {}
    }
    symbols.dedup();
    symbols
}
