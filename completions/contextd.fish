complete -c contextd -f

# Subcommands
complete -c contextd -n "__fish_use_subcommand" -a daemon -d "Run as a background service"
complete -c contextd -n "__fish_use_subcommand" -a mcp -d "Run as an MCP server"
complete -c contextd -n "__fish_use_subcommand" -a setup -d "Download embedding model"
complete -c contextd -n "__fish_use_subcommand" -a query -d "One-off semantic search"

# Global options
complete -c contextd -s c -l config -d "Path to configuration file" -r -F
complete -c contextd -s h -l help -d "Print help"
complete -c contextd -s V -l version -d "Print version"

# Query subcommand options
complete -c contextd -n "__fish_seen_subcommand_from query" -s l -l limit -d "Maximum results" -r
complete -c contextd -n "__fish_seen_subcommand_from query" -s s -l min-score -d "Minimum relevance score" -r
complete -c contextd -n "__fish_seen_subcommand_from query" -s a -l after -d "Filter by earliest modification time" -r
complete -c contextd -n "__fish_seen_subcommand_from query" -s b -l before -d "Filter by latest modification time" -r
