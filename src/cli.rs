use anyhow::{anyhow, Result};
use clap::Subcommand;
use clap_complete::Shell;

use crate::config::Config;
use crate::connect::ConnectScope;
use crate::indexer::embeddings::Embedder;
use crate::storage::db::{Database, SearchOptions};

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run as a daemon (default)
    Daemon {
        /// Exit automatically when stdin closes (useful when spawned by an editor extension)
        #[arg(long, default_value_t = false)]
        exit_on_stdin_eof: bool,
    },
    /// Run as an MCP server over stdio
    Mcp,
    /// Download and verify configured embedding model files
    Setup,
    /// Query the index using hybrid semantic + keyword search
    Query {
        query: String,
        /// Number of context lines to show before/after match
        #[arg(short = 'C', long, default_value_t = 0)]
        context: usize,
        /// Maximum number of results to return (1-100)
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        /// Minimum relevance score (0.0-1.0)
        #[arg(long)]
        min_score: Option<f32>,
        /// Only include files modified at or after this date (YYYY-MM-DD or UNIX timestamp)
        #[arg(long)]
        after: Option<String>,
        /// Only include files modified at or before this date (YYYY-MM-DD or UNIX timestamp)
        #[arg(long)]
        before: Option<String>,
        /// Filter by file extensions (e.g. --file-types rs,py)
        #[arg(long, value_delimiter = ',')]
        file_types: Option<Vec<String>>,
        /// Filter by path substrings (e.g. --paths src/,docs/)
        #[arg(long, value_delimiter = ',')]
        paths: Option<Vec<String>>,
        /// Retrieval mode: hybrid (default), vector, or fts
        #[arg(long, default_value = "hybrid")]
        mode: String,
        /// Emit results as JSON
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Configure MCP for compatible AI tools
    Connect {
        /// Configure all detected tools without prompting
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Configuration scope: user-global (default) or project-local
        #[arg(long, value_enum, default_value_t = ConnectScope::User)]
        scope: ConnectScope,
        /// Configure a specific tool by ID (e.g. claude-desktop, cursor, codex)
        #[arg(long)]
        tool: Option<String>,
    },
    /// Generate shell completion scripts
    Completions {
        /// Target shell
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// Parse a date (`YYYY-MM-DD`) or UNIX epoch timestamp (seconds) into UNIX seconds.
pub fn parse_timestamp_or_date(input: &str, end_of_day: bool) -> Result<u64> {
    let trimmed = input.trim();
    if let Ok(ts) = trimmed.parse::<u64>() {
        return Ok(ts);
    }

    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.len() == 3 {
        let year: i64 = parts[0]
            .parse()
            .map_err(|_| anyhow!("Invalid year in date '{}'", input))?;
        let month: u32 = parts[1]
            .parse()
            .map_err(|_| anyhow!("Invalid month in date '{}'", input))?;
        let day: u32 = parts[2]
            .parse()
            .map_err(|_| anyhow!("Invalid day in date '{}'", input))?;

        if !(1..=12).contains(&month) || !(1..=31).contains(&day) || year < 1970 {
            return Err(anyhow!(
                "Invalid date '{}' (expected YYYY-MM-DD >= 1970-01-01)",
                input
            ));
        }

        // Howard Hinnant's civil-from-days algorithm (UTC)
        let y = if month <= 2 { year - 1 } else { year };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = (y - era * 400) as u64;
        let m = month as u64;
        let d = day as u64;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days_since_epoch = era * 146097 + doe as i64 - 719468;
        if days_since_epoch < 0 {
            return Err(anyhow!("Date '{}' precedes UNIX epoch", input));
        }
        let mut secs = (days_since_epoch as u64) * 86_400;
        if end_of_day {
            secs += 86_399;
        }
        return Ok(secs);
    }

    Err(anyhow!(
        "Invalid timestamp '{}': expected UNIX epoch seconds or YYYY-MM-DD",
        input
    ))
}

pub async fn handle_setup(config: &Config) -> Result<()> {
    println!("Setting up model: {}", config.storage.model_type);
    println!(
        "Target directory: {:?}",
        crate::download::model_cache_dir(&config.storage.model_path, &config.storage.model_type)
    );

    crate::download::ensure_model_files(&config.storage.model_path, &config.storage.model_type)
        .await?;

    println!("Model setup complete.");
    Ok(())
}

pub struct QueryCliOptions {
    pub context_lines: usize,
    pub limit: usize,
    pub min_score: Option<f32>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub file_types: Option<Vec<String>>,
    pub paths: Option<Vec<String>>,
    pub mode: String,
    pub json: bool,
}

pub async fn handle_query(config: &Config, query: &str, cli_opts: QueryCliOptions) -> Result<()> {
    let start_time = match &cli_opts.after {
        Some(s) => Some(parse_timestamp_or_date(s, false)?),
        None => None,
    };
    let end_time = match &cli_opts.before {
        Some(s) => Some(parse_timestamp_or_date(s, true)?),
        None => None,
    };

    let mode = match cli_opts.mode.trim() {
        "" | "hybrid" => "hybrid",
        "vector" => "vector",
        "fts" => "fts",
        other => {
            return Err(anyhow!(
                "Unsupported --mode '{}': expected 'hybrid', 'vector', or 'fts'",
                other
            ));
        }
    };
    let emit_json = cli_opts.json;
    let storage_cfg = config.storage.clone();
    let query_owned = query.to_string();
    let limit = cli_opts.limit.clamp(1, 100);
    let context_lines = cli_opts.context_lines.min(20);

    let results = tokio::task::spawn_blocking(move || -> Result<_> {
        let options = SearchOptions {
            limit: Some(limit),
            start_time,
            end_time,
            file_types: cli_opts.file_types,
            paths: cli_opts.paths,
            min_score: cli_opts.min_score,
            context_lines: if context_lines > 0 {
                Some(context_lines)
            } else {
                None
            },
            ..Default::default()
        };

        if mode == "fts" {
            let spec = crate::indexer::embeddings::lookup_model_spec(&storage_cfg.model_type)?;
            let db = Database::open(&storage_cfg.db_path, spec.expected_dim, spec.id)?;
            return db.search_chunks_fts(&query_owned, &options);
        }

        let embedder = Embedder::new(&storage_cfg)?;
        let db = Database::open(
            &storage_cfg.db_path,
            embedder.embedding_dim(),
            embedder.model_type(),
        )?;
        let embedding = embedder.embed_query(&query_owned)?;

        if mode == "vector" {
            db.search_chunks_enhanced(&embedding, &options)
        } else {
            db.search_chunks_hybrid(&query_owned, &embedding, &options)
        }
    })
    .await??;

    if emit_json {
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    println!("Found {} results for '{}':", results.len(), query);
    for (i, res) in results.iter().enumerate() {
        let line_info = match (res.line_start, res.line_end) {
            (Some(s), Some(e)) => format!(" (lines {}-{})", s, e),
            _ => String::new(),
        };
        println!(
            "\n{}. {}{} (Score: {:.4})",
            i + 1,
            res.file_path,
            line_info,
            res.score
        );
        if let Some(before) = &res.context_before {
            for line in before.lines() {
                println!("   | {}", line);
            }
        }
        println!(
            "   > {}",
            res.content
                .replace('\n', " ")
                .chars()
                .take(160)
                .collect::<String>()
        );
        if let Some(after) = &res.context_after {
            for line in after.lines() {
                println!("   | {}", line);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timestamp_or_date() {
        assert_eq!(
            parse_timestamp_or_date("1700000000", false).unwrap(),
            1700000000
        );
        assert_eq!(parse_timestamp_or_date("1970-01-01", false).unwrap(), 0);
        assert_eq!(parse_timestamp_or_date("1970-01-01", true).unwrap(), 86399);
        assert_eq!(
            parse_timestamp_or_date("2024-12-01", false).unwrap(),
            1733011200
        );
        assert!(parse_timestamp_or_date("not-a-date", false).is_err());
    }
}
