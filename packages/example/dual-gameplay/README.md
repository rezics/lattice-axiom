# latticeaxiom-dual-fixture

Package-local headless D1 fixture for `@example/dual-gameplay`. Its manifest,
Nickel declaration, Rust crate, tests, and benchmarks share one package source
root, so code changes participate in the package source digest.

The shipped package exposes one data-only realization over `tests/fixtures`.
Its Rust crate remains developer conformance evidence for static and portable
implementations, but the package does not advertise a `SourceBuild` artifact.
A NativeStatic product still requires the lock-specific generated product
linker defined by ADR 0034; an `rlib` alone is not an activation-ready product
executable.

The portable path is intentionally an in-process generated-table harness. The
repository does not yet expose a reviewed safe native-library loader, so the
shipped package also does not advertise a PortableNative realization. This
fixture does not call `LoadLibrary`, `FreeLibrary`, `dlopen`, or `dlclose` and
contains no unsafe code. A production loader remains follow-up work.
