# latticeaxiom-launcher

Pure Rust process-transition contracts for the first-demo client. This crate
does not create a Bevy `App`, an event loop, a task runtime, or an operating
system process. It supplies the stable boundary that those adapters consume.

The V1 flow is deliberately fail-closed:

1. the current shell or world completes a typed shutdown barrier;
2. a canonical, checksummed `LaunchIntentV1` is written to a confined
   temporary file, synchronized, and atomically published;
3. the external launcher validates exact target/lock receipts and atomically
   changes the slot from `pending` to `claimed` before spawning;
4. a matching safe-bootstrap acknowledgement changes `claimed` to `consumed`;
5. any invalid intent or target failure is quarantined and may publish one
   canonical, checksummed recovery request, never the failed target again;
6. the process supervisor atomically acquires that exact recovery request and
   reconciles it after restart without creating a second physical child.

The store remains bounded to fixed literal slots and capped blobs. Publishing a
strictly newer generation makes the new `pending` bytes observable before
cleaning its exact terminal predecessor; that authenticated predecessor remains
available until claim, so a restart can derive the replay watermark without an
empty-slot window. A prior `claimed` or `consumed` child is reconciled by exact
intent checksum and durable supervisor ownership. A running child suppresses a
duplicate, an exited child may enter recovery, and unknown ownership fails
closed.

`claim_fresh_client_app_lease` is a process-global, non-resettable gate. Its
linear token is intentionally non-cloneable and non-serializable. A client
Bevy adapter must consume it before constructing the process's only fresh
`DefaultPlugins`/`EventLoop` application; headless `MinimalPlugins` instances
remain outside that policy.

The filesystem implementation accepts only an existing, absolute, canonical,
non-symlink root and uses compile-time literal slot names. Linux tests also
prove that a slot symlink cannot read outside the root. This confinement assumes
a launcher-owned private root without a hostile concurrent local writer;
protecting against pathname replacement races requires a directory-handle
platform adapter.

On Windows the store uses safe stable `OpenOptionsExt` to open the canonical
root with `FILE_WRITE_DATA`, shared read/write/delete access, and
`FILE_FLAG_BACKUP_SEMANTICS`; `File::sync_all` then delegates the namespace
flush to `FlushFileBuffers`. An open or flush failure is still exact-state
re-read and reported as `MutationDurability::Indeterminate` or
`PublishDisposition::PublishedIndeterminate`, which callers treat as committed
for retry/spawn suppression rather than as permission to create another child.

The automated corpus covers barrier, temporary write, file sync, atomic move,
spawn, boot, acknowledgement, child crash, claimed-intent launcher crash,
stale/corrupt/expired input, canonical byte and checksum validation,
idempotency, input bounds, quarantine, consumption, and recovery-loop
suppression.
