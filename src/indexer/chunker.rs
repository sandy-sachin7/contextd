use crate::config::{DEFAULT_CHUNK_OVERLAP, DEFAULT_MAX_CHUNK_CHARS};
use anyhow::{anyhow, Result};
use std::panic::{catch_unwind, AssertUnwindSafe};
use tree_sitter::Parser;

#[derive(Debug, Clone)]
pub struct Chunk {
    pub start: u64,
    pub end: u64,
    pub content: String,
    pub metadata: Option<String>,
}

pub fn chunk_by_type(content: &str, ext: &str) -> Result<Vec<Chunk>> {
    chunk_by_type_with_limits(content, ext, DEFAULT_MAX_CHUNK_CHARS, DEFAULT_CHUNK_OVERLAP)
}

pub fn chunk_by_type_with_limits(
    content: &str,
    ext: &str,
    max_chars: usize,
    overlap: usize,
) -> Result<Vec<Chunk>> {
    let raw = match ext.to_lowercase().as_str() {
        "rs" => chunk_rust(content)?,
        "py" => chunk_python(content)?,
        "js" | "jsx" | "mjs" | "cjs" => chunk_javascript(content)?,
        "ts" | "mts" | "cts" => chunk_typescript(content)?,
        "tsx" => chunk_tsx(content)?,
        "go" => chunk_go(content)?,
        "md" | "markdown" => chunk_markdown(content)?,
        _ => chunk_text(content)?,
    };
    Ok(enforce_chunk_budget(raw, max_chars, overlap))
}

/// Split any chunk exceeding `max_chars` into UTF-8-safe overlapping windows,
/// preferring newline boundaries while preserving byte offsets and metadata.
pub fn enforce_chunk_budget(chunks: Vec<Chunk>, max_chars: usize, overlap: usize) -> Vec<Chunk> {
    let max_chars = max_chars.max(64);
    let overlap = overlap.min(max_chars / 2);
    let mut out = Vec::with_capacity(chunks.len());

    for chunk in chunks {
        if chunk.content.chars().count() <= max_chars {
            if !chunk.content.trim().is_empty() {
                out.push(chunk);
            }
            continue;
        }

        let s = chunk.content.as_str();
        let char_indices: Vec<usize> = s.char_indices().map(|(i, _)| i).collect();
        let total_chars = char_indices.len();
        let mut char_pos = 0usize;

        while char_pos < total_chars {
            let mut end_char = (char_pos + max_chars).min(total_chars);
            let start_byte = char_indices[char_pos];
            let raw_end_byte = if end_char < total_chars {
                char_indices[end_char]
            } else {
                s.len()
            };

            // Try to snap backward to a newline boundary if it keeps at least half the window
            let mut end_byte = raw_end_byte;
            if end_char < total_chars {
                let slice = &s[start_byte..raw_end_byte];
                if let Some(nl_rel) = slice.rfind('\n') {
                    let candidate_byte = start_byte + nl_rel + 1;
                    let candidate_chars = s[start_byte..candidate_byte].chars().count();
                    if candidate_chars >= max_chars / 2 {
                        end_byte = candidate_byte;
                        end_char = char_pos + candidate_chars;
                    }
                }
            }

            let piece = &s[start_byte..end_byte];
            if !piece.trim().is_empty() {
                out.push(Chunk {
                    start: chunk.start + start_byte as u64,
                    end: chunk.start + end_byte as u64,
                    content: piece.to_string(),
                    metadata: chunk.metadata.clone(),
                });
            }

            if end_char >= total_chars {
                break;
            }
            let step = end_char
                .saturating_sub(char_pos)
                .saturating_sub(overlap)
                .max(1);
            char_pos += step;
        }
    }

    out
}

pub fn chunk_rust(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    parser.set_language(&language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse Rust code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    let mut pending_prefix_start: Option<usize> = None;

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();

        if matches!(kind, "line_comment" | "block_comment" | "attribute_item") {
            if pending_prefix_start.is_none() {
                pending_prefix_start = Some(child.start_byte());
            }
            continue;
        }

        if matches!(
            kind,
            "function_item"
                | "impl_item"
                | "struct_item"
                | "enum_item"
                | "mod_item"
                | "trait_item"
                | "const_item"
                | "static_item"
                | "type_item"
                | "macro_definition"
        ) {
            let chunk_start = pending_prefix_start.unwrap_or(child.start_byte());
            let chunk_end = child.end_byte();
            let chunk_content = &content[chunk_start..chunk_end];

            chunks.push(Chunk {
                start: chunk_start as u64,
                end: chunk_end as u64,
                content: chunk_content.to_string(),
                metadata: None,
            });

            pending_prefix_start = None;
        } else {
            pending_prefix_start = None;
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

/// Semantic chunking for Python using Tree-sitter
pub fn chunk_python(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_python::LANGUAGE.into();
    parser.set_language(&language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse Python code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        if matches!(
            kind,
            "function_definition" | "class_definition" | "decorated_definition"
        ) {
            chunks.push(Chunk {
                start: child.start_byte() as u64,
                end: child.end_byte() as u64,
                content: content[child.start_byte()..child.end_byte()].to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

/// Semantic chunking for JavaScript/JSX using Tree-sitter
pub fn chunk_javascript(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    parser.set_language(&language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse JavaScript code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        if matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "export_statement"
                | "lexical_declaration"
                | "expression_statement"
        ) {
            if kind == "expression_statement" && child.end_byte() - child.start_byte() < 50 {
                continue;
            }

            chunks.push(Chunk {
                start: child.start_byte() as u64,
                end: child.end_byte() as u64,
                content: content[child.start_byte()..child.end_byte()].to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

fn chunk_ts_with_language(content: &str, language: tree_sitter::Language) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    parser.set_language(&language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse TypeScript/TSX code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        if matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
                | "enum_declaration"
                | "export_statement"
                | "lexical_declaration"
        ) {
            chunks.push(Chunk {
                start: child.start_byte() as u64,
                end: child.end_byte() as u64,
                content: content[child.start_byte()..child.end_byte()].to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

/// Semantic chunking for TypeScript using Tree-sitter
pub fn chunk_typescript(content: &str) -> Result<Vec<Chunk>> {
    chunk_ts_with_language(content, tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
}

/// Semantic chunking for TSX (TypeScript + JSX) using Tree-sitter
pub fn chunk_tsx(content: &str) -> Result<Vec<Chunk>> {
    chunk_ts_with_language(content, tree_sitter_typescript::LANGUAGE_TSX.into())
}

/// Semantic chunking for Go using Tree-sitter
pub fn chunk_go(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_go::LANGUAGE.into();
    parser.set_language(&language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow!("Failed to parse Go code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        if matches!(
            kind,
            "function_declaration"
                | "method_declaration"
                | "type_declaration"
                | "const_declaration"
                | "var_declaration"
        ) {
            chunks.push(Chunk {
                start: child.start_byte() as u64,
                end: child.end_byte() as u64,
                content: content[child.start_byte()..child.end_byte()].to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

fn parse_fence_marker(trimmed: &str) -> Option<(char, usize)> {
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count >= 3 {
        Some((first, count))
    } else {
        None
    }
}

fn parse_atx_header(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let level = trimmed.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &trimmed[level..];
    // ATX headers require whitespace or end-of-line after '#'
    if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
        Some((level, rest.trim()))
    } else {
        None
    }
}

pub fn chunk_markdown(content: &str) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut current_chunk_start: usize = 0;
    let mut current_chunk_end: usize = 0;
    let mut header_stack: Vec<String> = Vec::new();
    let mut active_fence: Option<(char, usize)> = None;

    let mut byte_offset: usize = 0;
    for line in content.split_inclusive('\n') {
        let line_start = byte_offset;
        let line_end = byte_offset + line.len();
        byte_offset = line_end;

        let trimmed_start = line.trim_start();
        if let Some((fence_char, fence_len)) = active_fence {
            if let Some((c, len)) = parse_fence_marker(trimmed_start) {
                if c == fence_char && len >= fence_len && trimmed_start[len..].trim().is_empty() {
                    active_fence = None;
                }
            }
            current_chunk_end = line_end;
            continue;
        } else if let Some((c, len)) = parse_fence_marker(trimmed_start) {
            active_fence = Some((c, len));
            current_chunk_end = line_end;
            continue;
        }

        if let Some((level, title)) = parse_atx_header(line) {
            if current_chunk_end > current_chunk_start {
                let slice = &content[current_chunk_start..current_chunk_end];
                if !slice.trim().is_empty() {
                    let metadata = if !header_stack.is_empty() {
                        Some(serde_json::json!({ "headers": header_stack }).to_string())
                    } else {
                        None
                    };
                    chunks.push(Chunk {
                        start: current_chunk_start as u64,
                        end: current_chunk_end as u64,
                        content: slice.to_string(),
                        metadata,
                    });
                }
            }

            if level > header_stack.len() {
                header_stack.push(title.to_string());
            } else {
                header_stack.truncate(level - 1);
                header_stack.push(title.to_string());
            }

            current_chunk_start = line_start;
            current_chunk_end = line_end;
        } else {
            if current_chunk_end == current_chunk_start {
                current_chunk_start = line_start;
            }
            current_chunk_end = line_end;
        }
    }

    if current_chunk_end > current_chunk_start {
        let slice = &content[current_chunk_start..current_chunk_end];
        if !slice.trim().is_empty() {
            let metadata = if !header_stack.is_empty() {
                Some(serde_json::json!({ "headers": header_stack }).to_string())
            } else {
                None
            };
            chunks.push(Chunk {
                start: current_chunk_start as u64,
                end: current_chunk_end as u64,
                content: slice.to_string(),
                metadata,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

pub fn chunk_text(content: &str) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut start = 0usize;

    for paragraph in content.split("\n\n") {
        let len = paragraph.len();
        if !paragraph.trim().is_empty() {
            chunks.push(Chunk {
                start: start as u64,
                end: (start + len) as u64,
                content: paragraph.to_string(),
                metadata: None,
            });
        }
        start += len + 2;
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

pub fn chunk_pdf(path: &std::path::Path) -> Result<Vec<Chunk>> {
    let bytes = std::fs::read(path)?;
    chunk_pdf_bytes(&bytes, path)
}

pub fn chunk_pdf_bytes(bytes: &[u8], path: &std::path::Path) -> Result<Vec<Chunk>> {
    let extracted = catch_unwind(AssertUnwindSafe(|| {
        pdf_extract::extract_text_from_mem(bytes)
    }))
    .map_err(|_| anyhow!("PDF parser panicked while reading {:?}", path))??;

    let normalized = extracted.replace('\x0c', "\n\n");
    let mut chunks = Vec::new();
    let mut start = 0usize;

    for paragraph in normalized.split("\n\n") {
        let len = paragraph.len();
        let clean_para = paragraph.trim();
        if !clean_para.is_empty() {
            chunks.push(Chunk {
                start: start as u64,
                end: (start + len) as u64,
                content: clean_para.to_string(),
                metadata: None,
            });
        }
        start += len + 2;
    }

    Ok(enforce_chunk_budget(
        chunks,
        DEFAULT_MAX_CHUNK_CHARS,
        DEFAULT_CHUNK_OVERLAP,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text() {
        let content = "Para 1\n\nPara 2";
        let chunks = chunk_text(content).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].content, "Para 1");
        assert_eq!(chunks[1].content, "Para 2");
    }

    #[test]
    fn test_chunk_text_empty() {
        let content = "";
        let chunks = chunk_text(content).unwrap();
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_chunk_rust_preserves_attrs_docs_and_consts() {
        let content = r#"
/// Doc comment for Bar
#[derive(Debug, Clone)]
pub struct Bar {
    pub x: i32,
}

pub const MAX_SIZE: usize = 1024;
pub type MyResult<T> = Result<T, String>;

/// Helper function
#[inline]
fn foo() {
    println!("Hello");
}
"#;
        let chunks = chunk_rust(content).unwrap();
        assert_eq!(chunks.len(), 4);
        assert!(chunks[0].content.contains("/// Doc comment for Bar"));
        assert!(chunks[0].content.contains("#[derive(Debug, Clone)]"));
        assert!(chunks[0].content.contains("pub struct Bar"));
        assert!(chunks[1].content.contains("pub const MAX_SIZE"));
        assert!(chunks[2].content.contains("pub type MyResult"));
        assert!(chunks[3].content.contains("/// Helper function"));
        assert!(chunks[3].content.contains("#[inline]"));
        assert!(chunks[3].content.contains("fn foo"));
    }

    #[test]
    fn test_chunk_markdown_respects_code_fences_and_exact_offsets() {
        let content = "# Header 1\nSome text.\n```bash\n# This is a bash comment, not a header\necho hi\n```\n\n## Header 2\nMore text.\n";
        let chunks = chunk_markdown(content).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("# This is a bash comment"));
        assert_eq!(
            &content[chunks[0].start as usize..chunks[0].end as usize],
            chunks[0].content
        );
        assert_eq!(
            &content[chunks[1].start as usize..chunks[1].end as usize],
            chunks[1].content
        );
        assert!(chunks[1].content.starts_with("## Header 2"));
    }

    #[test]
    fn test_chunk_tsx_parses_jsx_syntax() {
        let content = r#"
export interface Props {
    title: string;
}

export function Card({ title }: Props) {
    return <div className="card"><h1>{title}</h1></div>;
}
"#;
        let chunks = chunk_by_type(content, "tsx").unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("interface Props"));
        assert!(chunks[1].content.contains("<div className=\"card\">"));
    }

    #[test]
    fn test_chunk_size_budget_splits_giant_items() {
        let giant_body = "let x = 1;\n".repeat(400); // > 4000 chars
        let content = format!("fn huge() {{\n{}}}\n", giant_body);
        let chunks = chunk_by_type_with_limits(&content, "rs", 500, 50).unwrap();
        assert!(chunks.len() > 5, "Giant function should be sub-split");
        for chunk in &chunks {
            assert!(
                chunk.content.chars().count() <= 500,
                "Chunk exceeded budget: {}",
                chunk.content.chars().count()
            );
        }
    }

    #[test]
    fn test_chunk_pdf_handles_corrupt_bytes_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let bad_pdf = dir.path().join("corrupt.pdf");
        std::fs::write(&bad_pdf, b"%PDF-1.4\nnot a valid pdf body\x00\xff").unwrap();
        let result = chunk_pdf(&bad_pdf);
        assert!(result.is_err());
    }

    #[test]
    fn test_chunk_python() {
        let content = r#"
def hello():
    print("Hello")

class Greeter:
    def greet(self):
        return "Hi"
"#;
        let chunks = chunk_python(content).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("def hello"));
        assert!(chunks[1].content.contains("class Greeter"));
    }

    #[test]
    fn test_chunk_javascript() {
        let content = r#"
function greet() {
    console.log("Hello");
}

class Person {
    constructor(name) {
        this.name = name;
    }
}
"#;
        let chunks = chunk_javascript(content).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("function greet"));
        assert!(chunks[1].content.contains("class Person"));
    }

    #[test]
    fn test_chunk_typescript() {
        let content = r#"
interface User {
    name: string;
    age: number;
}

function getUser(): User {
    return { name: "Alice", age: 30 };
}

type ID = string | number;
"#;
        let chunks = chunk_typescript(content).unwrap();
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().any(|c| c.content.contains("interface User")));
        assert!(chunks
            .iter()
            .any(|c| c.content.contains("function getUser")));
    }

    #[test]
    fn test_chunk_go() {
        let content = r#"
package main

func hello() {
    fmt.Println("Hello")
}

type Person struct {
    Name string
    Age  int
}

func (p Person) Greet() string {
    return "Hi " + p.Name
}
"#;
        let chunks = chunk_go(content).unwrap();
        assert!(chunks.len() >= 2);
        assert!(chunks.iter().any(|c| c.content.contains("func hello")));
        assert!(chunks.iter().any(|c| c.content.contains("type Person")));
    }
}
