use criterion::{black_box, Criterion, criterion_group, criterion_main};
use glyim_bench::fixtures::FixtureGenerator;
use glyim_compiler::queries::QueryPipeline;
use glyim_compiler::pipeline::PipelineConfig;
use std::time::Duration;

fn bench_incremental(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(15));

    // Generate a fixture with 100 functions
    let fixture = FixtureGenerator::single_file(100);
    let source = &fixture.source;
    let path = &fixture.path;

    // Full build benchmark
    group.bench_function("full_build_100fn", |b| {
        b.iter(|| {
            let cache_dir = tempfile::tempdir().unwrap();
            let config = PipelineConfig::default();
            let mut qp = QueryPipeline::new(cache_dir.path(), config);
            let _ = qp.compile(black_box(source), black_box(path));
        });
    });

    // Incremental edit benchmark: change one function only
    group.bench_function("incremental_edit_1fn", |b| {
        // First, do a full build to populate the cache
        let cache_dir = tempfile::tempdir().unwrap();
        let config = PipelineConfig::default();
        let mut qp = QueryPipeline::new(cache_dir.path(), config);
        qp.compile(source, path).expect("initial build");

        // Create an edited source (change function 42)
        let edited = source.replacen("fn fn_42", "fn fn_42_edited", 1);
        let edit_path = fixture.path.with_extension("edited.g");
        std::fs::write(&edit_path, &edited).expect("write edited");

        b.iter(|| {
            let mut qp = QueryPipeline::new(cache_dir.path(), PipelineConfig::default());
            let _ = qp.compile(black_box(&edited), black_box(&edit_path));
        });
    });

    group.finish();
}

criterion_group!(benches, bench_incremental);
criterion_main!(benches);
