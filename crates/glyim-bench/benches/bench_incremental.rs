use criterion::{black_box, Criterion, criterion_group, criterion_main};
use glyim_bench::fixtures::FixtureGenerator;
use glyim_compiler::queries::QueryPipeline;
use glyim_compiler::pipeline::{PipelineConfig, run_jit};
use std::time::Duration;

fn bench_full_build(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_build");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for n in &[10, 50, 100, 500] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = fixture.source.clone();
        let path = fixture.path.clone();
        group.bench_function(format!("{n}fn"), move |b| {
            b.iter(|| {
                let cache_dir = tempfile::tempdir().unwrap();
                let mut qp = QueryPipeline::new(cache_dir.path(), PipelineConfig::default());
                let _ = qp.compile(black_box(&source), black_box(&path));
            });
        });
    }
    group.finish();
}

fn bench_incremental_edit(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    let fixture = FixtureGenerator::single_file(100);
    let source = fixture.source.clone();
    let path = fixture.path.clone();

    // Full build
    group.bench_function("full_build_100fn", |b| {
        b.iter(|| {
            let cache_dir = tempfile::tempdir().unwrap();
            let mut qp = QueryPipeline::new(cache_dir.path(), PipelineConfig::default());
            let _ = qp.compile(black_box(&source), black_box(&path));
        });
    });

    // Edit one function
    let edited1 = source.replacen("fn fn_42", "fn fn_42_edited", 1);
    group.bench_function("edit_1fn", move |b| {
        b.iter(|| {
            let cache_dir = tempfile::tempdir().unwrap();
            let mut qp = QueryPipeline::new(cache_dir.path(), PipelineConfig::default());
            let _ = qp.compile(black_box(&edited1), black_box(&path));
        });
    });

    // Edit five functions
    let edited5 = {
        let mut s = source.clone();
        for i in &[10, 20, 30, 40, 50] {
            let old = format!("fn fn_{i} ");
            let new = format!("fn fn_{i}_edited ");
            s = s.replace(&old, &new);
        }
        s
    };
    group.bench_function("edit_5fn", move |b| {
        b.iter(|| {
            let cache_dir = tempfile::tempdir().unwrap();
            let mut qp = QueryPipeline::new(cache_dir.path(), PipelineConfig::default());
            let _ = qp.compile(black_box(&edited5), black_box(&path));
        });
    });

    group.finish();
}

fn bench_parser(c: &mut Criterion) {
    let mut group = c.benchmark_group("parser");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));

    for n in &[100, 500, 1000] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = fixture.source.clone();
        group.bench_with_input(format!("{n}fn"), &source, |b, s| {
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

    let source = "fn main() -> i64 { (1 + 2) * 3 + 4 * 5 + 6 + 7 * 8 }";
    let source = source.to_string();
    group.bench_function("arithmetic_expr", move |b| {
        b.iter(|| {
            let parse_out = glyim_parse::parse(black_box(&source));
            let mut interner = parse_out.interner;
            let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
            if let Some(glyim_hir::HirItem::Fn(f)) = hir.items.first() {
                let _ = glyim_egraph::optimize_fn(f, &[], &interner, &Default::default());
            }
        });
    });
    group.finish();
}

fn bench_jit_exec(c: &mut Criterion) {
    let mut group = c.benchmark_group("jit");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("simple_main", |b| {
        b.iter(|| { let _ = run_jit(black_box("main = () => 42")); });
    });
    group.bench_function("arithmetic", |b| {
        b.iter(|| { let _ = run_jit(black_box("main = () => { let x = 10; let y = 20; x + y }")); });
    });
    group.bench_function("loop", |b| {
        b.iter(|| { let _ = run_jit(black_box("main = () => { let mut i = 0; while i < 100 { i = i + 1 }; i }")); });
    });
    group.finish();
}

criterion_group!(full_build, bench_full_build);
criterion_group!(incremental, bench_incremental_edit);
criterion_group!(parser, bench_parser);
criterion_group!(egraph, bench_egraph);
criterion_group!(jit, bench_jit_exec);

criterion_main!(full_build, incremental, parser, egraph, jit);
