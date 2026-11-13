# Local authenticated dashboard

Build with `cargo build --locked --release`. Use an existing registry, such as the one produced by the research-loop smoke workflow. Initialize a new owner-only password-hash file using a password read securely from stdin; plaintext passwords are not written to that file or command arguments.

Example in Bash from the repository root:

```sh
read -r -s -p 'Choose local admin password (at least 16 bytes): ' factory_password
printf '%s' "$factory_password" | cargo run --locked -- auth-init state/admin.json
unset factory_password
cargo run --locked -- serve state/research.sqlite state/admin.json 3000
```

The file must be new and mode 0600. Existing files are never overwritten. The server binds only to loopback, accepts unprivileged ports (or zero for test allocation), and prints its actual port. Stop it with Ctrl-C for graceful shutdown. Sessions are in memory and require sign-in again after a server restart; research and results remain in SQLite. This local HTTP mode uses HttpOnly/SameSite=Strict cookies. Public exposure and HTTPS reverse-proxy/secure-cookie configuration are not implemented or authorized by this runbook.

Implemented: authenticated persisted overview and recent experiments, run pause/resume/stop requests, instrument filtering, full strategy/profile/cost and trade receipts, hypothetical equity plots, numerical training diagnostics, exact raw JSON exports and data provenance. Controls do not spawn a missing coordinator: start the actual research process using the separate loop runbook. Pending scientific evidence keeps all qualification boards empty. The UI never writes official performance metrics.

Security controls tested: Argon2id password hashing, owner-only credential file, bounded random sessions, CSRF token checks, strict host/origin checks, no arbitrary generated-code/shell execution, request-body and login/read-rate limits, no-store responses, restrictive content security policy, and rejected metrics-write attempts. This is not a completed supply-chain or penetration audit. Embedded assets require no production Node service.

Experiment history now pages through all retained records,25 per page, with Newer/Older/Newest navigation and backend instrument filtering. Data provenance reflects only manifests in the currently displayed page. Trade/protocol/run pagination, data import/approval/refresh UI, backup/restore UI, parameter/worker management, complete independent validation/qualified ranking and richer diagnostics remain unfinished. Large monetary values stay exact in exported integer JSON receipts; the browser declines unsafe-range numeric display rather than inventing precision.

Browser acceptance:

```sh
cargo build --locked
npm ci --ignore-scripts
npm run test:browser
```

In the cloud sandbox use `npm --cache /workspace/.npm-cache ci --ignore-scripts`. Tests use an installed Chromium when available; otherwise install the pinned Playwright Chromium with its supported installer. Node is required only for development tests. The test starts actual Rust processes, builds synthetic persisted results, checks desktop/mobile workflows and server restart, then stops its own processes. Ignored screenshots are written to `state/browser/`. No public preview is created.

Generic diagnostic JSON guards parsed integers outside the exact display range and directs users to the raw receipt. Currency formatting inside the safe range uses integer quotient/remainder, including negative values; raw receipt downloads preserve the original HTTP bytes. Actual extreme-loss/browser and safe-boundary regressions cover these paths.

Authenticated `GET /api/experiment-history` accepts optional `limit` (1–100, default25), `instrument` (one supported symbol) and `before` (positive SQLite rowid expressed as decimal text). It returns `{rows,next_cursor}`; follow a non-null cursor with the same filter. The cursor is a string to preserve SQLite i64 precision. Newer inserts do not shift an older boundary; this is a live history view rather than an immutable database snapshot. Invalid/unknown query fields are refused. The previous `/api/experiments` newest100 array endpoint is retained for compatibility. No write operation or new scientific permission is introduced. Browser acceptance covers110 actual CLI receipts, insertion between pages, older symbol records, mobile/desktop navigation and a stale delayed filter response.
