# latticeaxiom-start-ui

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
