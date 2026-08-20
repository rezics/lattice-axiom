# Controlled worker fixtures

This directory begins the protocol-level R0 corpus. The import-free Tool
package is evaluated through the shared embedded controller and the public
controller CLI by `r0_worker_parity.rs`. Both paths
must return identical canonical typed response bytes for success and for an
atomic controller-fatal capability mismatch.

The fixture does not claim production `r0@1` capability. The temporary trusted
adapter rejects any static import after immutable source-closure preflight and
before Nickel evaluation. Production remains fail-closed until the
source-table-only loader, hard OS memory containment, and Nickel call-frame
meter are installed and recorded.
