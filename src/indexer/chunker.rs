use anyhow::Result;
use tree_sitter::{Language, Parser};

pub struct Chunk {
    pub start: u64,
    pub end: u64,
    pub content: String,
}

pub fn chunk_by_type(content: &str, ext: &str) -> Result<Vec<Chunk>> {
    match ext {
        "rs" => chunk_rust(content),
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

    // Iterate over top-level nodes
    for child in root_node.children(&mut cursor) {
        let kind = child.kind();
        // We want to chunk by major definitions
        if matches!(
            kind,
            "function_item" | "impl_item" | "struct_item" | "enum_item" | "mod_item" | "trait_item"
        ) {
            let start_byte = child.start_byte() as u64;
            let end_byte = child.end_byte() as u64;
            let chunk_content = &content[child.start_byte()..child.end_byte()];

            chunks.push(Chunk {
                start: start_byte,
                end: end_byte,
                content: chunk_content.to_string(),
            });
        }
    }

    // If no chunks found (e.g. script or just comments), fallback to text chunking
    if chunks.is_empty() && !content.trim().is_empty() {
        return chunk_text(content);
    }

    Ok(chunks)
}

pub fn chunk_markdown(content: &str) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut current_chunk_start = 0;
    let mut current_chunk_content = String::new();

    for (i, line) in content.lines().enumerate() {
        // Check for headers
        if line.starts_with("#") {
            // If we have accumulated content, push it as a chunk
            if !current_chunk_content.trim().is_empty() {
                chunks.push(Chunk {
                    start: current_chunk_start as u64,
                    end: (current_chunk_start + current_chunk_content.len()) as u64,
                    content: current_chunk_content.clone(),
                });
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
        chunks.push(Chunk {
            start: current_chunk_start as u64,
            end: (current_chunk_start + current_chunk_content.len()) as u64,
            content: current_chunk_content,
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
        });

        start += len + 2; // content + \n\n
    }

    Ok(chunks)
}

pub fn chunk_pdf(path: &std::path::Path) -> Result<Vec<Chunk>> {
    let bytes = std::fs::read(path)?;
    let content = pdf_extract::extract_text_from_mem(&bytes)?;

    // Reuse text chunking logic for now
    chunk_text(&content)
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
}
