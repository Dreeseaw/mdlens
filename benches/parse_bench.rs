use criterion::{criterion_group, criterion_main, Criterion};

fn parse_bench(c: &mut Criterion) {
    let mut content = String::new();
    content.push_str("# Overview\n\nThis is a large document for benchmarking.\n\n");
    for i in 1..=50 {
        content.push_str(&format!("## Section {}\n\n", i));
        for j in 1..=5 {
            content.push_str(&format!(
                "### Subsection {}.{}\n\nLorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n\n",
                i, j
            ));
        }
    }

    c.bench_function("parse_large_markdown", |b| {
        b.iter(|| mdlens::parse::parse_markdown_str("benchmark.md", &content).unwrap());
    });
}

fn search_bench(c: &mut Criterion) {
    let mut content = String::new();
    content.push_str("# Overview\n\nThis is a large document for benchmarking.\n\n");
    for i in 1..=50 {
        content.push_str(&format!("## Section {}\n\n", i));
        for j in 1..=5 {
            content.push_str(&format!(
                "### Subsection {}.{}\n\nLorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam.\n\n",
                i, j
            ));
        }
    }

    let _doc = mdlens::parse::parse_markdown_str("benchmark.md", &content).unwrap();
    let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

    c.bench_function("search_regex_in_large_doc", |b| {
        b.iter(|| {
            let regex = regex::Regex::new("(?i)lorem").unwrap();
            let mut count = 0;
            for line in &lines {
                if regex.is_match(line) {
                    count += 1;
                }
            }
            count
        });
    });
}

fn token_estimate_bench(c: &mut Criterion) {
    let content = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(1000);

    c.bench_function("estimate_tokens_16kb", |b| {
        b.iter(|| mdlens::tokens::estimate_tokens(&content));
    });
}

criterion_group!(benches, parse_bench, search_bench, token_estimate_bench);
criterion_main!(benches);
