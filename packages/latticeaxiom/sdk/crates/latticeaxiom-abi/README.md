# latticeaxiom-abi

`latticeaxiom-abi` is the engine-independent foundation for Portable Native
ABI 0.1. It contains generated C and Rust wire layouts, stable status codes,
domain-separated interface identities, canonical table descriptors, bounded
batch/command/message validators, and the staged-write recovery policy.

The typed schema in `schema/portable_native_abi_v0_1.rs` is the sole layout
source. `build.rs` generates both `repr(C)` Rust bindings and
`include/latticeaxiom_abi.h`; a regeneration test requires the committed C
header to match byte-for-byte.

This crate does not load libraries, expose Bevy types, dereference foreign
pointers, catch operating-system faults, or claim sandboxing. The future
loader/SDK crate must establish readable fixed headers before copying metadata
into these validators, retain native libraries for the process lifetime, and
implement the lifecycle and callback boundary around these DTOs.
