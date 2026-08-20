//! Reproducible CPU diagnostics for deterministic world-generation primitives.

use criterion::{Criterion, criterion_group, criterion_main};
use latticeaxiom_worldgen::WorldSeedV1;

fn seed_derivation(c: &mut Criterion) {
    c.bench_function("world_seed_integer_v1", |bencher| {
        bencher.iter(|| WorldSeedV1::from_integer(42));
    });
}

criterion_group!(benches, seed_derivation);
criterion_main!(benches);
