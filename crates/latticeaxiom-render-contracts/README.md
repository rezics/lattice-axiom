# latticeaxiom-render-contracts

Pure data schemas and deterministic validation for the first Lattice Axiom
render contract. This crate compiles logical SSA resource plans, validates
exactly-one provider selection and device fallback, and atomically validates the
bounded D5 portable command vocabulary.

It intentionally has no Bevy, wgpu, window, device, or GPU submission dependency.
The Bevy host adapter owns semantic-slot mapping, physical resources, barriers,
uploads, pipelines, renderer schedules, and submission. Headless profiles can
still compile plans and validate declarations and command fixtures.

The contract implements the five Core 3D slots and hard command limits accepted
by ADR 0029. Device-dependent provider and physical-format choices are
presentation receipts and never authoritative world state.
