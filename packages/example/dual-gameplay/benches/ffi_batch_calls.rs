//! D1 static, generated-batch, and invalid per-entity call diagnostics.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use latticeaxiom_dual_fixture::{Realization, ffi_batch_call_diagnostic, run_realization};

fn execution_benchmarks(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("d1-dual-gameplay");
    group.sample_size(10);
    for entity_count in [1_usize, 64, 256, 1_000, 10_000] {
        group.throughput(Throughput::Elements(
            u64::try_from(entity_count).unwrap_or(u64::MAX),
        ));
        group.bench_with_input(
            BenchmarkId::new("static-direct", entity_count),
            &entity_count,
            |bencher, &count| {
                bencher
                    .iter(|| black_box(run_realization(Realization::StaticDirect, 1, count).ok()));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("generated-portable-batch", entity_count),
            &entity_count,
            |bencher, &count| {
                bencher
                    .iter(|| black_box(run_realization(Realization::PortableBatch, 1, count).ok()));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("per-entity-ffi-counterexample", entity_count),
            &entity_count,
            |bencher, &count| {
                bencher.iter(|| {
                    let diagnostic = ffi_batch_call_diagnostic(count);
                    for call in 0..diagnostic.per_entity_counterexample {
                        black_box(call);
                    }
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, execution_benchmarks);
criterion_main!(benches);
