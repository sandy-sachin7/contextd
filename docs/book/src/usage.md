# Usage

contextd has four subcommands: `daemon`, `mcp`, `setup`, and `query`.

- **Daemon mode** runs as a background service, watches files, and serves the REST API.
- **MCP mode** runs as an MCP stdio server for AI tool integration.
- **Setup mode** downloads embedding models from HuggingFace.
- **Query mode** performs one-off semantic searches from the terminal.

The `daemon` subcommand is the default if none is specified.
