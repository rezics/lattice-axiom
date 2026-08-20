# Lattice Axiom territory planning

`latticeaxiom-territory` owns the engine-independent D7 planning contracts for
multi-scale surface territories, underground cave-topology delegation,
bounded detail and constraint contributions, abstract hydrology, cave portal
adjacency, and planning-cell generation epochs.

The crate does not generate voxels, realize fluids, render diagnostics, open a
world writer, or provide a task runtime. Callers schedule its immutable pure
queries through Bevy.

## Contract guarantees

- Atlas plans use at least three validated coarse-to-fine scales and produce
  order-independent queries, canonical serialization, hashes, and statistics.
- Each active dimension resolves exactly one generation coordinator. Every
  terrain and cave-topology ownership domain resolves exactly one primary
  owner, with explicit parent inheritance.
- Detail and constraint contributors declare typed compositors, finite spatial
  influence, and hard work and memory budgets.
- The cave skeleton contains a dimension default plus bounded underground
  children. Portals validate adjacency, tangent, clearance, and abstract
  hydrology compatibility without realizing fluid state.
- A generation epoch freezes independently on each 2D planning cell at first
  durable materialization. Cross-epoch transitions require direction-independent
  adapter and portal receipts before materialization.

## Automated gates

```text
cargo test -p latticeaxiom-territory --lib --tests
cargo clippy -p latticeaxiom-territory --all-targets --no-deps -- -D warnings
cargo bench -p latticeaxiom-territory --bench atlas --no-run
```

The Criterion target records cold compilation of 1,000 candidates, cached
query throughput, a caller-side parallel-query baseline, and canonical plan
size.
