use criterion::{black_box, Criterion, criterion_group, criterion_main};
use glyim_bench::fixtures::FixtureGenerator;
use glyim_compiler::queries::QueryPipeline;
use glyim_compiler::pipeline::{PipelineConfig, run_jit};
use std::time::Duration;

// ── Helper: create a fresh QueryPipeline for a bench ──
fn bench_query_pipeline(c: &mut Criterion, name: &str, source: &str, path: &std::path::Path) {
    c.bench_function(name, |b| {
        b.iter(|| {
            let cache_dir = tempfile::tempdir().unwrap();
            let mut qp = QueryPipeline::new(cache_dir.path(), PipelineConfig::default());
            let _ = qp.compile(black_box(source), black_box(path));
        });
    });
}

fn bench_jit(c: &mut Criterion, name: &str, source: &str) {
    c.bench_function(name, |b| {
        b.iter(|| {
            let _ = run_jit(black_box(source));
        });
    });
}

// ── Benchmarks ──

fn bench_full_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_build");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    let sizes = [10, 50, 100, 500];
    for &n in &sizes {
        let fixture = FixtureGenerator::single_file(n);
        bench_query_pipeline(
            &mut group,
            &format!("full_build/{}fn", n),
            &fixture.source,
            &fixture.path,
        );
    }
    group.finish();
}

fn bench_incremental_edit(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    // 100‑function fixture as baseline
    let fixture = FixtureGenerator::single_file(100);
    let source = &fixture.source;
    let path = &fixture.path;

    // Full build
    bench_query_pipeline(&mut group, "incremental/full_build_100fn", source, path);

    // Edit one function
    let edited1 = source.replacen("fn fn_42", "fn fn_42_edited", 1);
    let edit_path = fixture.path.with_extension("edited.g");
    std::fs::write(&edit_path, &edited1).unwrap();
    bench_query_pipeline(&mut group, "incremental/edit_1fn_after_full", &edited1, &edit_path);

    // Edit five functions
    let mut edited5 = source.clone();
    for i in &[10, 20, 30, 40, 50] {
        let old_name = format!("fn fn_{} ", i);
        let new_name = format!("fn fn_{}_edited ", i);
        edited5 = edited5.replace(&old_name, &new_name);
    }
    let edit5_path = fixture.path.with_extension("edited5.g");
    std::fs::write(&edit5_path, &edited5).unwrap();
    bench_query_pipeline(&mut group, "incremental/edit_5fn_after_full", &edited5, &edit5_path);

    group.finish();
}

fn bench_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));

    for n in &[100, 500, 1000] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = &fixture.source;
        group.bench_with_input(format!("parse/{}fn", n), source, |b, s| {
            b.iter(|| {
                let _ = glyim_parse::parse(black_box(s));
            });
        });
    }
    group.finish();
}

fn bench_egraph(c: &mut Criterion) {
    let mut group = c.benchmark_group("egraph");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));

    // simple expression with many arithmetic ops – triggers e‑graph
    let source = r#"fn main() -> i64 { (1 + 2) * 3 + 4 * 5 + 6 + 7 * 8 }"#;
    group.bench_function("optimize/arithmetic_expr", |b| {
        b.iter(|| {
            let parse_out = glyim_parse::parse(black_box(source));
            let mut interner = parse_out.interner;
            let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
            let types = vec![glyim_hir::types::HirType::Int];
            // run e‑graph on main function
            if let glyim_hir::HirItem::Fn(f) = &hir.items[0] {
                let _ = glyim_egraph::optimize_fn(&f, &types, &interner, &Default::default());
            }
        });
    });

    group.finish();
}

fn bench_jit_exec(c: &mut Criterion) {
    let mut group = c.benchmark_group("jit");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    bench_jit(&mut group, "jit/simple_main", "main = () => 42");
    bench_jit(&mut group, "jit/arithmetic", "main = () => { let x = 10; let y = 20; x + y }");
    bench_jit(&mut group, "jit/loop", "main = () => { let mut i = 0; while i < 100 { i = i + 1 }; i }");

    group.finish();
}

criterion_group!(full_build, bench_full_build);
criterion_group!(incremental, bench_incremental_edit);
criterion_group!(parser, bench_parser);
criterion_group!(egraph, bench_egraph);
criterion_group!(jit, bench_jit_exec);

criterion_main!(full_build, incremental, parser, egraph, jit);
