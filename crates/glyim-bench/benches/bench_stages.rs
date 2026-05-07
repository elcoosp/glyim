use criterion::{black_box, Criterion, criterion_group, criterion_main};
use glyim_bench::fixtures::FixtureGenerator;
use glyim_compiler::pipeline::{PipelineConfig, compile_source_to_hir};
use std::time::Duration;

/// Benchmarks that measure individual pipeline stages,
/// useful for identifying bottlenecks.

// ── Parse only (no lowering/typeck/codegen) ──
fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_parse");
    group.sample_size(30);
    group.measurement_time(Duration::from_secs(5));

    for n in &[10, 100, 1000] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = fixture.source.clone();
        group.bench_function(format!("{n}fn"), move |b| {
            b.iter(|| {
                let _ = glyim_parse::parse(black_box(&source));
            });
        });
    }
    group.finish();
}

// ── Parse + Lower to HIR (no typecheck) ──
fn bench_lower(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_lower");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));

    for n in &[10, 100, 500] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = fixture.source.clone();
        group.bench_function(format!("{n}fn"), move |b| {
            b.iter(|| {
                let parse_out = glyim_parse::parse(black_box(&source));
                let mut interner = parse_out.interner;
                let _ = glyim_hir::lower(&parse_out.ast, &mut interner);
            });
        });
    }
    group.finish();
}

// ── Full parsing + lowering + type check (stop before codegen) ──
fn bench_typecheck(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_typecheck");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(5));

    for n in &[10, 50, 100] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = fixture.source.clone();
        group.bench_function(format!("{n}fn"), move |b| {
            b.iter(|| {
                let parse_out = glyim_parse::parse(black_box(&source));
                let mut interner = parse_out.interner;
                let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
                let mut typeck = glyim_typeck::TypeChecker::new(interner);
                let _ = typeck.check(&hir);
            });
        });
    }
    group.finish();
}

// ── Codegen (LLVM IR generation) ──
fn bench_codegen(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_codegen");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    for n in &[10, 50, 100] {
        let fixture = FixtureGenerator::single_file(*n);
        let source = fixture.source.clone();
        let path = fixture.path.clone();
        group.bench_function(format!("{n}fn"), move |b| {
            b.iter(|| {
                let parse_out = glyim_parse::parse(black_box(&source));
                let mut interner = parse_out.interner;
                let hir = glyim_hir::lower(&parse_out.ast, &mut interner);
                let ctx = inkwell::context::Context::create();
                let mut cg = glyim_codegen_llvm::CodegenBuilder::new(&ctx, interner, vec![])
                    .build()
                    .unwrap();
                let _ = cg.generate(&hir);
            });
        });
    }
    group.finish();
}

// ── Semantic hash computation ──
fn bench_semantic_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("stage_semantic_hash");
    group.sample_size(50);
    group.measurement_time(Duration::from_secs(5));

    // Use a fixed set of functions
    let fixture = FixtureGenerator::single_file(100);
    let source = fixture.source.clone();
    let parse_out = glyim_parse::parse(&source);
    let mut interner = parse_out.interner;
    let hir = glyim_hir::lower(&parse_out.ast, &mut interner);

    group.bench_function("100_items", |b| {
        b.iter(|| {
            for item in &hir.items {
                let _ = glyim_hir::semantic_hash::semantic_hash_item(black_box(item), black_box(&interner));
            }
        });
    });
    group.finish();
}

criterion_group!(
    stages,
    bench_parse,
    bench_lower,
    bench_typecheck,
    bench_codegen,
    bench_semantic_hash
);

criterion_main!(stages);
