# Smart Chunking

contextd uses Tree-sitter AST parsing for semantic code splitting:

| Language | Strategy | AST Nodes Captured |
|----------|----------|-------------------|
| Rust | Tree-sitter | `function_item`, `impl_item`, `struct_item`, `enum_item`, `mod_item`, `trait_item` |
| Python | Tree-sitter | `function_definition`, `class_definition`, `decorated_definition` |
| JavaScript | Tree-sitter | `function_declaration`, `class_declaration`, `export_statement`, `lexical_declaration`, `expression_statement` |
| TypeScript | Tree-sitter | Same as JS + `interface_declaration`, `type_alias_declaration` |
| Go | Tree-sitter | `function_declaration`, `method_declaration`, `type_declaration`, `const_declaration`, `var_declaration` |
| Markdown | Header-based | Sections by heading hierarchy |
| PDF | Page/form-feed | Paragraph split |
| Other | Paragraph split | By blank lines |
