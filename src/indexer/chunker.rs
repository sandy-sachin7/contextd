use anyhow::Result;
use tree_sitter::Parser;

pub struct Chunk {
    pub start: u64,
    pub end: u64,
    pub content: String,
    pub metadata: Option<String>,
}

pub fn chunk_by_type(content: &str, ext: &str) -> Result<Vec<Chunk>> {
    match ext {
        "rs" => chunk_rust(content),
        "py" => chunk_python(content),
        "js" | "jsx" => chunk_javascript(content),
        "ts" | "tsx" => chunk_typescript(content),
        "go" => chunk_go(content),
        "md" | "markdown" => chunk_markdown(content),
        _ => chunk_text(content),
    }
}

pub fn chunk_rust(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language = tree_sitter_rust::language();
    parser.set_language(language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse Rust code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    let mut pending_comments_start: Option<usize> = None;

    // Iterate over top-level nodes
    for child in root_node.children(&mut cursor) {
        let kind = child.kind();

        if kind == "line_comment" || kind == "block_comment" {
            if pending_comments_start.is_none() {
                pending_comments_start = Some(child.start_byte());
            }
            continue;
        }

        // We want to chunk by major definitions
        if matches!(
            kind,
            "function_item" | "impl_item" | "struct_item" | "enum_item" | "mod_item" | "trait_item"
        ) {
            let start_byte = pending_comments_start.unwrap_or(child.start_byte()) as u64;
            let end_byte = child.end_byte() as u64;

            // Ensure we capture from the start of comments if present
            let chunk_start = pending_comments_start.unwrap_or(child.start_byte());
            let chunk_content = &content[chunk_start..child.end_byte()];

            chunks.push(Chunk {
                start: start_byte,
                end: end_byte,
                content: chunk_content.to_string(),
                metadata: None,
            });

            pending_comments_start = None;
        } else {
            // Reset comments if we hit something else (like whitespace or other nodes)
            // But wait, whitespace isn't a node usually.
            // If we hit something else that isn't a comment or a target item, we should probably clear comments?
            // E.g. a macro_invocation or use_declaration.
            // Yes, clear comments.
            pending_comments_start = None;
        }
    }

    // If no chunks found (e.g. script or just comments), fallback to text chunking
    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

/// Semantic chunking for Python using Tree-sitter
pub fn chunk_python(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language = tree_sitter_python::language();
    parser.set_language(language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse Python code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        // Chunk by function definitions, class definitions, and decorated definitions
        if matches!(
            kind,
            "function_definition" | "class_definition" | "decorated_definition"
        ) {
            let start_byte = child.start_byte() as u64;
            let end_byte = child.end_byte() as u64;
            let chunk_content = &content[child.start_byte()..child.end_byte()];

            chunks.push(Chunk {
                start: start_byte,
                end: end_byte,
                content: chunk_content.to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

/// Semantic chunking for JavaScript using Tree-sitter
pub fn chunk_javascript(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language = tree_sitter_javascript::language();
    parser.set_language(language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse JavaScript code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        // Chunk by functions, classes, and exports
        if matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "export_statement"
                | "lexical_declaration"
                | "expression_statement"
        ) {
            // For expression_statement, only include if it's a significant size
            if kind == "expression_statement" && child.end_byte() - child.start_byte() < 50 {
                continue;
            }

            let start_byte = child.start_byte() as u64;
            let end_byte = child.end_byte() as u64;
            let chunk_content = &content[child.start_byte()..child.end_byte()];

            chunks.push(Chunk {
                start: start_byte,
                end: end_byte,
                content: chunk_content.to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

/// Semantic chunking for TypeScript using Tree-sitter
pub fn chunk_typescript(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language = tree_sitter_typescript::language_typescript();
    parser.set_language(language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse TypeScript code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        // Chunk by functions, classes, interfaces, types, and exports
        if matches!(
            kind,
            "function_declaration"
                | "class_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
                | "export_statement"
                | "lexical_declaration"
        ) {
            let start_byte = child.start_byte() as u64;
            let end_byte = child.end_byte() as u64;
            let chunk_content = &content[child.start_byte()..child.end_byte()];

            chunks.push(Chunk {
                start: start_byte,
                end: end_byte,
                content: chunk_content.to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

/// Semantic chunking for Go using Tree-sitter
pub fn chunk_go(content: &str) -> Result<Vec<Chunk>> {
    let mut parser = Parser::new();
    let language = tree_sitter_go::language();
    parser.set_language(language)?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse Go code"))?;
    let root_node = tree.root_node();
    let mut chunks = Vec::new();
    let mut cursor = root_node.walk();

    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        // Chunk by functions, methods, types, and const/var declarations
        if matches!(
            kind,
            "function_declaration"
                | "method_declaration"
                | "type_declaration"
                | "const_declaration"
                | "var_declaration"
        ) {
            let start_byte = child.start_byte() as u64;
            let end_byte = child.end_byte() as u64;
            let chunk_content = &content[child.start_byte()..child.end_byte()];

            chunks.push(Chunk {
                start: start_byte,
                end: end_byte,
                content: chunk_content.to_string(),
                metadata: None,
            });
        }
    }

    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

pub fn chunk_markdown(content: &str) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut current_chunk_start = 0;
    let mut current_chunk_content = String::new();
    let mut header_stack: Vec<String> = Vec::new();

    for line in content.lines() {
        // Check for headers
        if line.starts_with("#") {
            // If we have accumulated content, push it as a chunk
            if !current_chunk_content.trim().is_empty() {
                let metadata = if !header_stack.is_empty() {
                    Some(serde_json::json!({ "headers": header_stack }).to_string())
                } else {
                    None
                };

                chunks.push(Chunk {
                    start: current_chunk_start as u64,
                    end: (current_chunk_start + current_chunk_content.len()) as u64,
                    content: current_chunk_content.clone(),
                    metadata,
                });
            }

            // Update header stack
            let level = line.chars().take_while(|c| *c == '#').count();
            let title = line[level..].trim().to_string();

            if level > header_stack.len() {
                header_stack.push(title);
            } else {
                header_stack.truncate(level - 1);
                header_stack.push(title);
            }

            // Start new chunk
            current_chunk_start += current_chunk_content.len() + 1; // +1 for newline (approx)
            current_chunk_content = line.to_string();
            current_chunk_content.push('\n');
        } else {
            current_chunk_content.push_str(line);
            current_chunk_content.push('\n');
        }
    }

    // Push last chunk
    if !current_chunk_content.trim().is_empty() {
        let metadata = if !header_stack.is_empty() {
            Some(serde_json::json!({ "headers": header_stack }).to_string())
        } else {
            None
        };

        chunks.push(Chunk {
            start: current_chunk_start as u64,
            end: (current_chunk_start + current_chunk_content.len()) as u64,
            content: current_chunk_content,
            metadata,
        });
    }

    // Fallback if no headers found
    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

pub fn chunk_text(content: &str) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut start = 0;

    // Simple paragraph splitter for Phase 1
    for paragraph in content.split("\n\n") {
        let len = paragraph.len() as u64;
        if len == 0 {
            start += 2; // Skip double newline
            continue;
        }

        chunks.push(Chunk {
            start,
            end: start + len,
            content: paragraph.to_string(),
            metadata: None,
        });

        start += len + 2; // content + \n\n
    }

    Ok(chunks)
}

pub fn chunk_pdf(path: &std::path::Path) -> Result<Vec<Chunk>> {
    let bytes = std::fs::read(path)?;
    let content = pdf_extract::extract_text_from_mem(&bytes)?;

    let mut chunks = Vec::new();
    let mut start = 0;

    // Split by double newlines (paragraphs)
    // Also consider page breaks as boundaries
    let _splits = content.split(|c| c == '\n' || c == '\x0c');
    // Actually, splitting by \n might be too aggressive if it's just line wrapping.
    // Let's split by \n\n or \x0c

    // Simple approach: Normalize \x0c to \n\n, then split by \n\n
    let normalized = content.replace('\x0c', "\n\n");

    for paragraph in normalized.split("\n\n") {
        let len = paragraph.len() as u64;
        if len == 0 {
            start += 2;
            continue;
        }

        // Clean up whitespace
        let clean_para = paragraph.trim();
        if clean_para.is_empty() {
            start += len + 2;
            continue;
        }

        chunks.push(Chunk {
            start,
            end: start + len,
            content: clean_para.to_string(),
            metadata: None, // Could add page number if we tracked it
        });

        start += len + 2;
    }

    Ok(chunks)
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
    fn test_chunk_rust() {
        let content = r#"
fn foo() {
    println!("Hello");
}

struct Bar {
    x: i32,
}
"#;
        let chunks = chunk_rust(content).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("fn foo"));
        assert!(chunks[1].content.contains("struct Bar"));
    }

    #[test]
    fn test_chunk_markdown() {
        let content = r#"# Header 1
Some text.

## Header 2
More text.
"#;
        let chunks = chunk_markdown(content).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("# Header 1"));
        assert!(chunks[1].content.contains("## Header 2"));
    }

    #[test]
    fn test_chunk_pdf_logic() {
        // Simulate PDF content with Form Feed characters
        let content = "Page 1 content\x0cPage 2 content\x0cPage 3 content";

        let mut chunks = Vec::new();
        let mut start = 0;
        for page in content.split('\x0c') {
            let len = page.len() as u64;
            chunks.push(Chunk {
                start,
                end: start + len,
                content: page.to_string(),
                metadata: None,
            });
            start += len + 1;
        }

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].content, "Page 1 content");
        assert_eq!(chunks[1].content, "Page 2 content");
        assert_eq!(chunks[2].content, "Page 3 content");
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
