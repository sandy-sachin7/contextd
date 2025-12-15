use anyhow::Result;

pub struct Chunk {
    pub start: u64,
    pub end: u64,
    pub content: String,
}

pub fn chunk_text(content: &str) -> Result<Vec<Chunk>> {
    let mut chunks = Vec::new();
    let mut start = 0;

    // Simple paragraph splitter for Phase 1
    for (_i, paragraph) in content.split("\n\n").enumerate() {
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
}
