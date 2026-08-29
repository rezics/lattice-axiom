# Development observability

The interactive client uses Bevy's `LogPlugin` as its only in-process tracing
subscriber. Development builds additionally install Bevy's frame-time, entity,
process CPU, and process-memory diagnostic plugins. Diagnostics are sampled by
Bevy and logged at a bounded one-second interval; application code must not emit
per-frame or per-entity `INFO` events. Bevy 0.19 disables its system-information
collector when dynamic linking is active, so the ordinary fast development
profile reports frame and entity diagnostics while CPU and memory remain
available to compatible statically linked client profiles.

## Local Web viewer

`task dev` relocks the development product, launches Logdy on
`http://127.0.0.1:8080`, and streams the combined client stdout and stderr to its
Web UI. This includes fail-closed bootstrap messages written before Bevy can
install `LogPlugin`. `task logs:open` opens the UI, `task logs:install` installs
the reviewed binary without launching the client, and `task dev:plain` preserves
the direct no-viewer development path.

The sidecar binds both its UI and input socket to localhost, disables analytics
and update checks, retains at most 10,000 messages in memory, and persists JSONL
under ignored `run/logs/` with 100 MiB file rotation. Each launch prunes old
rotations beyond the five newest files. Override its two local ports with
`LATTICEAXIOM_LOG_UI_PORT` and `LATTICEAXIOM_LOG_INPUT_PORT`; values outside
`1024..=65535` fail before a process starts.

The launcher owns both process lifecycles instead of asking Logdy to own the
client. That preserves the client's exit status and lets Ctrl+C stop the client
process group before the Logdy sidecar is terminated. If the Web stream fails
after startup, the client continues and its console output remains visible.

## Upstream decision record

The local viewer is Logdy 0.17.1 at revision
`dd9a1c03694bd6ccfad05c1ad2cb0a5ce3c580eb`, licensed Apache-2.0. The reviewed
manifest in `scripts/development/logdy-v0.17.1.json` records exact sizes and
SHA-256 digests for Windows, Linux, and macOS x86-64/AArch64 release artifacts.
The installer always verifies both fields before executing a cached or newly
downloaded binary. Relevant upstream material:

- <https://github.com/logdyhq/logdy-core/tree/v0.17.1>
- <https://logdy.dev/docs/explanation/command-modes>
- <https://logdy.dev/docs/reference/cli>
- <https://logdy.dev/docs/explanation/features/file-rotation>

Logdy is a developer convenience, not a production log store or alerting
system. A future multi-process or remote operational deployment should ship
structured files with an external collector into a retained store such as Loki;
it must not add another global subscriber or a project-owned async runtime to
the Bevy process.
