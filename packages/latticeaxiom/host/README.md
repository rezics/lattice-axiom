# `@latticeaxiom/host`

Owns `latticeaxiom-host`: lock verification, compiled receipt validation and
access to frozen package artifacts before a Bevy application is activated.
It depends on generic package and launcher contracts, and has no Terrenia,
terrain presentation, inventory UI, or game-specific startup dependency.

Implementation entry and membership are declared in `latticeaxiom-package.toml`.
Public APIs are documented alongside the code. The Terrenia application owns
its Bevy setup and gameplay integration under `@terrenia/client` and consumes
these same verification types through its normal Rust dependency.
