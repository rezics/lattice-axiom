# `@latticeaxiom/kernel`

Owns `latticeaxiom-compose`, `latticeaxiom-core`, `latticeaxiom-packages`, `latticeaxiom-registration`.

Implementation entry and membership are declared in `latticeaxiom-package.toml`.
Public APIs and tests are maintained beside the code.

The [ecosystem direction](../../../docs/ecosystem-direction.md) defines the
product objective and compatibility gates. The kernel's part of the
[package interface design](../sdk/docs/package-interfaces.md) is resolution,
provider/instance binding and activation evidence; package authors own domain
APIs. Current native assembly is documented in [package builds](docs/package-build.md).
