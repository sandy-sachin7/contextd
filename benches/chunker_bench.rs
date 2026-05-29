use criterion::{criterion_group, criterion_main, Criterion};

fn large_rust_code() -> String {
    let mut code = String::new();
    for i in 0..50 {
        code.push_str(&format!(
            r#"
/// Documentation for function {i}
pub fn function_{i}(param: i32) -> i32 {{
    let result = param * 2;
    println!("Function {i}: {{result}}");
    result
}}

struct Struct_{i} {{
    field_{i}: i32,
}}

impl Struct_{i} {{
    pub fn new(val: i32) -> Self {{
        Self {{ field_{i}: val }}
    }}

    pub fn get(&self) -> i32 {{
        self.field_{i}
    }}
}}
"#,
        ));
    }
    code
}

fn large_python_code() -> String {
    let mut code = String::new();
    for i in 0..50 {
        code.push_str(&format!(
            r#"
def function_{i}(param: int) -> int:
    \"\"\"Documentation for function {i}."\"\"
    result = param * 2
    print(f"Function {i}: {{result}}")
    return result


class Class_{i}:
    \"\"\"A test class."\"\"

    def __init__(self, val: int):
        self.field = val

    def get(self) -> int:
        return self.field
"#,
        ));
    }
    code
}

fn large_js_code() -> String {
    let mut code = String::new();
    for i in 0..50 {
        code.push_str(&format!(
            r#"
function function_{i}(param) {{
    const result = param * 2;
    console.log(`Function {i}: ${{result}}`);
    return result;
}}

class Class_{i} {{
    constructor(val) {{
        this.field = val;
    }}

    get() {{
        return this.field;
    }}
}}
"#,
        ));
    }
    code
}

fn large_ts_code() -> String {
    let mut code = String::new();
    for i in 0..50 {
        code.push_str(&format!(
            r#"
interface Interface_{i} {{
    field: number;
    name: string;
}}

function function_{i}(param: number): number {{
    const result = param * 2;
    console.log(`Function {i}: ${{result}}`);
    return result;
}}
"#,
        ));
    }
    code
}

fn large_go_code() -> String {
    let mut code = "package main\n\nimport \"fmt\"\n\n".to_string();
    for i in 0..50 {
        code.push_str(&format!(
            r#"
type Struct_{i} struct {{
    Field int
}}

func (s *Struct_{i}) Method_{i}() int {{
    return s.Field * 2
}}

func function_{i}(param int) int {{
    result := param * 2
    fmt.Printf("Function {i}: %d\n", result)
    return result
}}
"#,
        ));
    }
    code
}

fn large_markdown() -> String {
    let mut md = String::new();
    for i in 0..50 {
        md.push_str(&format!(
            r#"# Section {i}

This is section {i} with some content. Lorem ipsum dolor sit amet,
consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut
labore et dolore magna aliqua.

## Subsection {i}.1

More detailed content here. Includes multiple paragraphs.

## Subsection {i}.2

Even more content for benchmarking purposes.

"#,
        ));
    }
    md
}

fn bench_chunk_rust(c: &mut Criterion) {
    let code = large_rust_code();
    c.bench_function("chunk_rust_50_items", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_rust(&code))
    });
}

fn bench_chunk_python(c: &mut Criterion) {
    let code = large_python_code();
    c.bench_function("chunk_python_50_items", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_python(&code))
    });
}

fn bench_chunk_javascript(c: &mut Criterion) {
    let code = large_js_code();
    c.bench_function("chunk_javascript_50_items", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_javascript(&code))
    });
}

fn bench_chunk_typescript(c: &mut Criterion) {
    let code = large_ts_code();
    c.bench_function("chunk_typescript_50_items", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_typescript(&code))
    });
}

fn bench_chunk_go(c: &mut Criterion) {
    let code = large_go_code();
    c.bench_function("chunk_go_50_items", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_go(&code))
    });
}

fn bench_chunk_markdown(c: &mut Criterion) {
    let md = large_markdown();
    c.bench_function("chunk_markdown_50_sections", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_markdown(&md))
    });
}

fn bench_chunk_dispatch(c: &mut Criterion) {
    let code = large_rust_code();
    c.bench_function("chunk_dispatch_rust", |b| {
        b.iter(|| contextd::indexer::chunker::chunk_by_type(&code, "rs"))
    });
}

criterion_group!(
    benches,
    bench_chunk_rust,
    bench_chunk_python,
    bench_chunk_javascript,
    bench_chunk_typescript,
    bench_chunk_go,
    bench_chunk_markdown,
    bench_chunk_dispatch,
);
criterion_main!(benches);
