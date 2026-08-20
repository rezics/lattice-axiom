# latticeaxiom-dual-fixture

Headless D1 fixture for `@example/dual-gameplay`. It exercises one gameplay
row kernel through static direct and portable batch-table reference paths.

The portable path is intentionally an in-process generated-table harness. The
repository does not yet expose a reviewed safe native-library loader, so this
fixture does not call `LoadLibrary`, `FreeLibrary`, `dlopen`, or `dlclose` and
contains no unsafe code. A production loader remains follow-up work.
