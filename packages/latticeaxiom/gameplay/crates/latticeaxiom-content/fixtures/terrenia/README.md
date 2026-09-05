# Terrenia content-contract fixtures

These files exercise the D9 Rust schema and the accepted Terrenia identity
inventory. They are test data, not a second authored package, RegistrationImage,
or balance source of truth.

- `expected-block-ids.txt` records the 72 exact block identities required by
  the accepted catalog. State combinations never add identities.
- `representative-catalog-v1.json` contains only representative rows: air, an
  axis block, a facing/lit block, both fluid identities, both biome identities,
  and explicit Role-to-block bindings. It proves that the schema can grow to
  the full inventory without fabricating the still-owner-managed 72-row values.

Later package integration should compare the shipped registration closure to
the inventory and feed authored canonical rows into this crate. It must not
copy values back into this fixture as an alternative package definition.
