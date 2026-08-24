# @latticeaxiom/front-end

This logical package owns the colocated latticeaxiom-start-ui Cargo crate.
The package name and crate name are intentionally independent identifiers;
Cargo.toml, data, src, and tests are part of the package source receipt.

The shipped realization remains frozen data, including the package-local
client-shell descriptor. The Rust implementation is compiled by the repository
workspace today, but this package does not advertise a native SourceBuild:
per-package frozen staging cannot yet supply its workspace-inherited platform
dependency closure. A native realization must wait for that closure to be
receipt-injected and independently buildable from CAS.

Headless semantic contracts for the package-driven client shell, world list,
quick-create flow, settings transaction surface, loading stages, accessibility,
and process-restart launch handoff.

This crate deliberately contains no renderer, Bevy `App`, arbitrary widget
tree, or Feathers dependency. A later engine-client adapter may project the
semantic tree into Bevy UI while retaining these command and accessibility
contracts.

Automated evidence covers state, ordering, focus, semantic actions, IME
capability declarations, and numeric layout reachability. No visual,
pixel-comparison, GPU, or operating-system screen-reader evidence has been
collected yet.
