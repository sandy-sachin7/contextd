# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_contextd_global_optspecs
    string join \n c/config= trust-workspace-plugins h/help V/version
end

function __fish_contextd_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_contextd_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_contextd_using_subcommand
    set -l cmd (__fish_contextd_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c contextd -n "__fish_contextd_needs_command" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_needs_command" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_needs_command" -s h -l help -d 'Print help'
complete -c contextd -n "__fish_contextd_needs_command" -s V -l version -d 'Print version'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "daemon" -d 'Run as a daemon (default)'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "mcp" -d 'Run as an MCP server over stdio'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "setup" -d 'Download and verify configured embedding model files'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "query" -d 'Query the index using hybrid semantic + keyword search'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "connect" -d 'Configure MCP for compatible AI tools'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "completions" -d 'Generate shell completion scripts'
complete -c contextd -n "__fish_contextd_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c contextd -n "__fish_contextd_using_subcommand daemon" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_using_subcommand daemon" -l exit-on-stdin-eof -d 'Exit automatically when stdin closes (useful when spawned by an editor extension)'
complete -c contextd -n "__fish_contextd_using_subcommand daemon" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_using_subcommand daemon" -s h -l help -d 'Print help'
complete -c contextd -n "__fish_contextd_using_subcommand mcp" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_using_subcommand mcp" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_using_subcommand mcp" -s h -l help -d 'Print help'
complete -c contextd -n "__fish_contextd_using_subcommand setup" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_using_subcommand setup" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_using_subcommand setup" -s h -l help -d 'Print help'
complete -c contextd -n "__fish_contextd_using_subcommand query" -s C -l context -d 'Number of context lines to show before/after match' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -s l -l limit -d 'Maximum number of results to return (1-100)' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -l min-score -d 'Minimum relevance score (0.0-1.0)' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -l after -d 'Only include files modified at or after this date (YYYY-MM-DD or UNIX timestamp)' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -l before -d 'Only include files modified at or before this date (YYYY-MM-DD or UNIX timestamp)' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -l file-types -d 'Filter by file extensions (e.g. --file-types rs,py)' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -l paths -d 'Filter by path substrings (e.g. --paths src/,docs/)' -r
complete -c contextd -n "__fish_contextd_using_subcommand query" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_using_subcommand query" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_using_subcommand query" -s h -l help -d 'Print help'
complete -c contextd -n "__fish_contextd_using_subcommand connect" -l scope -d 'Configuration scope: user-global (default) or project-local' -r -f -a "user\t'Configure user-global AI tool settings (default)'
project\t'Configure project-local AI tool settings in the current directory'"
complete -c contextd -n "__fish_contextd_using_subcommand connect" -l tool -d 'Configure a specific tool by ID (e.g. claude-desktop, cursor, codex)' -r
complete -c contextd -n "__fish_contextd_using_subcommand connect" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_using_subcommand connect" -l all -d 'Configure all detected tools without prompting'
complete -c contextd -n "__fish_contextd_using_subcommand connect" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_using_subcommand connect" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c contextd -n "__fish_contextd_using_subcommand completions" -s c -l config -d 'Path to the configuration file' -r -F
complete -c contextd -n "__fish_contextd_using_subcommand completions" -l trust-workspace-plugins -d 'Explicitly trust and execute [plugins] from a workspace-local contextd.toml'
complete -c contextd -n "__fish_contextd_using_subcommand completions" -s h -l help -d 'Print help'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "daemon" -d 'Run as a daemon (default)'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "mcp" -d 'Run as an MCP server over stdio'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "setup" -d 'Download and verify configured embedding model files'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "query" -d 'Query the index using hybrid semantic + keyword search'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "connect" -d 'Configure MCP for compatible AI tools'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "completions" -d 'Generate shell completion scripts'
complete -c contextd -n "__fish_contextd_using_subcommand help; and not __fish_seen_subcommand_from daemon mcp setup query connect completions help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
