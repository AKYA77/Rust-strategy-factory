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

Training lineage is available inside experiment detail: paginated stored diagnoses, comparable review links and an immutable report preview. `Export training evidence` downloads the exact response text, retaining i128 integers; the preview guards unsafe JS integers. Proposals remain UNEVALUATED and reviews DESCRIPTIVE_TRAINING_ONLY. No view executes children or changes qualification. Large report previews scroll locally; full experiment identities wrap on mobile.

Authenticated `GET /api/training-diagnoses?parent=EXPERIMENT_ID` and `GET /api/training-reviews?parent=DIAGNOSIS_ID` return `{rows,next_cursor}` metadata pages. Optional positive decimal-string `before` and `limit`1–25(default10) bound traversal; unknown fields/invalid identities are refused. `GET /api/training-diagnoses/ID` and `GET /api/training-reviews/ID` resolve full reports through existing immutable integrity readers, including parent/result/evaluation links. A tampered report fails closed; absent detail returns404. These are historical evidence reads, not independent validation or adaptation authorization.

## Exact trade event IDs

The authenticated detail response adds `trade_event_ids` decimal strings for the first500 visible trades, derived from the verified immutable result's original u64 values. The table renders these strings rather than rounded JavaScript numbers. Older responses without valid exact strings use a safe-integer guard and direct users to the raw receipt for unsafe values. The original result remains unchanged; `result_hash` covers that stored result, and exports retain its exact numeric values alongside the derived display fields.

Actual browser acceptance imports/replays a synthetic trade with entry18446744073709551614 and exit18446744073709551615, verifies both distinct decimal strings at desktop/mobile widths, safe-boundary fallback behavior and byte-for-byte raw export. The regression failed before the fix. These display/API changes preserve the complete compiled compute/version identity; no simulation is rerun or financial result altered.

## Independent data review

After provisioning the independent authority and machine admissions described in `dataset-catalog.md`, an operator may enable review workflows:

```sh
rust-strategy-factory serve registry.sqlite owner-auth.secret 8080 catalog.sqlite review-config.json
```

The bounded configuration selects an `authority` file and1..64 `artifacts`, each with `machine_id`, `archive` and `dataset` paths. Paths resolve relative to the configuration file. These server-owned paths are never selected by browser input or exported. The authority is reloaded for each operation; removal, unsafe permissions, corruption or policy rotation cannot retain cached approvals. Existing `serve` invocations remain supported; without review configuration, approval mutations are unavailable.

Inspect a catalog receipt to browse25-decision history pages, download the exact response and select its configured artifact admission. Revalidation is explicit and recomputes the current archive/dataset/calendar plus five scoped approval gates. A failure clears the displayed live result; historical consistency is not current eligibility. Decisions require the existing owner session/CSRF guards plus independently provisioned reviewer credentials on each submission. Owner credentials cannot substitute for independent review, and passwords are cleared after submission. Scope/expiry, synthetic/profile restrictions and latest-head fencing are enforced by the same durable backend as the CLI. Older-page submission is disabled until the latest history is loaded. Rejections/revocations retain evidence and prior decisions; no history-reset endpoint exists.

Review history: authenticated GET `/api/data-review/{catalog-identity}` with optional `before` decimal sequence and `limit`1..25. Live gate: GET `/api/data-eligibility/{configured-machine-id}`. Decision: POST `/api/data-review`, JSON `reviewer`, `password`, and the existing typed decision `request`. Existing8KiB body, host/origin/session/CSRF and request limits remain; reviewer authentication adds8 attempts/minute and a single bounded blocking-work slot shared with artifact verification. Secrets and provisioning grants are never returned. Backend scientific protocols remain blocked for real qualification prerequisites; no dashboard action can approve synthetic data or manufacture a qualified strategy.

Registered synthetic corpus jobs expose a Recovery action in experiment history. It opens verified checkpoint metadata and exact export, plus authenticated pause/resume/permanent-stop controls. No lease token or account snapshot is returned. Resume changes durable control state and requires a running worker; the browser does not start a process. Completed and stopped jobs disable controls. The API uses existing session, CSRF and origin enforcement. Chromium exercises desktop/mobile state, restart persistence, immutable stop and unchanged checkpoint hashes when held.

Run and protocol histories now use authenticated keyset pages rather than the old100-row browsing cap. Each page accepts a positive decimal `before` row cursor and a1–100 `limit` (default25); returned cursors remain exact strings. Run counters are decimal strings in the paged API. Protocol pages verify the immutable definition identity before reporting availability. Newer/older/newest controls preserve selected pages during refresh and run controls. Request tickets discard late older-page responses. The legacy overview/protocol endpoints remain compatible; `overview?runs=false` avoids reading redundant run statuses when dedicated pages are used.

The four Chromium suites include105+ disposable records, concurrent inserts, exact integers above2^53, invalid-query/read-only refusals, delayed responses and desktop/mobile controls. These fixture records do not grant scientific approval or start coordinators.

Trade detail uses authenticated100-row pages bound to the immutable result hash. Next/previous navigation reaches all resolved trades, with full-u64 entry/exit IDs supplied as decimal strings on every page. Invalid and foreign-receipt cursors refuse. The exact full export remains unchanged. This API still loads and verifies the complete stored receipt before slicing; it is not a large-artifact streaming implementation. Late experiment/trade responses cannot replace the currently selected receipt or export.

Realized-equity charts visit every trade for exact-safe-integer checks and ending sums, retain chronological bucket extrema in at most1002 rendered points and avoid argument-spread limits on long series. A range outside exact display limits remains an explicit export-only guard. Chart sampling is display-only; it does not alter financial receipts or calculate intratrade/account drawdown.

Runtime resource cards read authenticated `/api/resources`: current dashboard process RSS, host available memory and available parallelism, preserving exact decimal strings. The panel does not inventory separate research processes or claim OS enforcement. Observation failures remain unavailable rather than zero. The API is read-only and retains session/host/origin protections.

Generated corpus searches have a separate bounded inventory (global64-search quota), verified status snapshots, exact exports and authenticated pause/resume/permanent-stop controls. Cards distinguish generated/completed/error slots from simulations and show actual coordinator-lease activity. The browser starts no worker process. Search controls and metadata are separate from materialized research runs and supplied corpus batches. Corrupt queue metadata refuses verified status/export; stop and exhausted states disable controls.

Paused inactive generated searches expose Requeue eligible error slots when the frozen queue has capacity and a slot has remaining bounded requeues. The control retains stored prior-error history and keeps exact keys/child leases. Cards show requeue-record counts and snapshots export prior errors even after current errors clear. Running/stopped/exhausted/active-coordinator states refuse requeue. The browser starts no execution process.

Receipt storage cards read authenticated backend metadata/physical file counts. Inventory does not validate all content. `registry-check` verifies referenced receipt hashes and summaries; missing/corrupt files refuse experiment detail and full export. New receipts above256 KiB retain exact public exports in a content-addressed sibling store; page controls still bind the full receipt hash. Keep the store with every backup/restore/rename. No dashboard endpoint accepts result metrics or deletes receipt evidence. See DECISIONS/0013-file-backed-result-receipts.md for quotas and fault limits.

Foreground supervision sessions display verified invocation observations, independently of source queue controls. Inspect a session to export its exact receipt/status snapshot. Request session stop asks that invocation to exit at its next safe checkpoint; it starts no process and does not permanently stop its source. Finished or previously requested records disable the action. An unclosed record does not establish PID liveness. Refresh the selected snapshot to see later observations; verification failures disable controls and exports.

Owned child process history exposes parent invocation observations and terminal report digests. Export is the bounded registry observation, not the complete child report. Retain the original `corpus-supervise-owned` output separately when required. Unclosed parent/child records establish neither liveness nor safe PID ownership; use the process/service you actually own for operational intervention. Corruption refuses verified history/export.

Select a frozen protocol to inspect exact OOS counters, conditional evidence and held criteria, with independent blockers shown separately. Export preserves the exact raw report, including integer values beyond JavaScript's safe range. Changing selection or refreshing disables the prior export while verification is pending. Corrupt/incomplete reports cannot enable export. `NUMERICAL_EVIDENCE_ONLY` and conditional criteria never mean independently qualified; reset-fold net is not full-period compounded equity.

Managed registry snapshots now have authenticated create, inventory, inspect, verify, exact public-receipt export and new-label restore controls. Inspect metadata first, explicitly verify contents, then submit a distinct safe destination label. Restore binds the freshly reviewed manifest digest; selection changes or failed verification disable it. A successful restore leaves the primary service untouched and requires separate operator cutover. Pending operations use bounded background slots; busy actions refuse while health remains available. No API accepts an arbitrary path or downloads private registry bytes. See DECISIONS/0036-authenticated-recovery-workers.md.

Frozen corpus campaigns display declared slots, distinct experiment identities and completed-region progress. Inspect a campaign to download its exact stored scope/progress snapshot and use parent pause/resume/permanent-stop controls. Requests compare the state version you inspected; a refusal requires refreshing before retrying. Pending requests, changed selection and corrupt metadata disable stale exports/controls. Source holds remain separate; campaign resume starts no process. Execute through the explicit foreground CLI after reviewing current input eligibility. Metadata inspection alone does not establish live eligibility or qualification.

Campaign rotation cards show the fair cursor, published visits and linked observed/unknown costs for each frozen member. Inspect a rotation for exact snapshot export and parent pause/resume/permanent-stop controls. Pause on a scheduler-idle record asserts an operator hold; stale reviewed states refuse and require refresh. Member/source holds remain independent, and the browser starts no processes. Member metadata availability is not current data eligibility. Corrupt rotation metadata clears verified controls/exports.

Campaign detail now offers the full retained visit ledger in32-row ascending pages. Previous/next/first/refresh keep committed page ownership; exact page exports are disabled while loading or when verification fails. Counts and observed/unknown costs cover the complete retained campaign ledger. A valid page is not current source approval or worker liveness.

## Import activation lifecycle and operations panels

An operator may enable bounded import/coverage/audit workflows with an operations configuration:

```sh
rust-strategy-factory serve registry.sqlite owner-auth.secret 8080 catalog.sqlite --operations operations-config.json
rust-strategy-factory serve registry.sqlite owner-auth.secret 8080 --operations operations-config.json
```

The bounded configuration declares0..16 `imports` (`name`, `directory`) and0..16 `coverage` targets (`name`, `dataset`, `start_ms`, `end_exclusive_ms`, `gap_threshold_ms`), at least one target overall. Names are1..64 lowercase alphanumeric/hyphen characters and unique across both lists; paths resolve relative to the configuration file and are never selected by browser input or exported. Invalid configurations refuse startup. Without a configured catalog, import inventory/verification and coverage observations still work, but activation is refused.

Import inventory (`GET /api/imports`) re-verifies every published chunk through the same checkpoint validation as `import-status`: complete/in-progress states, exact decimal-string event/chunk counts, source archive hash, and explicit `not_initialized` or `refused` observations with the exact redacted refusal reason. `GET /api/imports/{name}` returns the full verified progress receipt plus retained operator decisions; export preserves the exact raw bytes. Tampered or missing chunks refuse inventory detail, export and activation; nothing is deleted or silently repaired.

Activation (`POST /api/imports/{name}/activate`) requires the owner session/CSRF/origin guards plus an explicit confirmation body: `expected_archive_hash` and `expected_events` must match the freshly verified checkpoint. It then re-assembles the exact import and registers a local catalog observation through the existing catalog module (idempotent by dataset identity). Activation is not approval: the declared class stays declared metadata, and no calendar/profile/legal/scientific gate, research eligibility or qualification is granted. Each activation/refusal appends a self-hashed receipt to `activation-decisions.jsonl` inside the import directory (at most1024 retained decisions, each8KiB); a corrupt history refuses further actions and preserves evidence. Refusal (`POST /api/imports/{name}/refuse`) records a bounded operator reason, verifies the archive hash when the checkpoint still verifies, and otherwise retains the exact verification obstacle as an `unverified` refusal. Refusal changes no artifact, checkpoint or catalog entry.

Coverage observations (`GET /api/coverage`, `GET /api/coverage/{name}`) run the existing bounded coverage report on operator-configured artifacts and windows, with at most100 gap details and explicit truncation flags. Empty months and elapsed gaps remain observations, not proof of missing source ranges; no completeness, approval or eligibility is established. A missing or corrupt configured artifact refuses the report.

The registry integrity panel (`GET /api/registry-audit`) runs the same read-only integrity verification as `registry-check` at request time and returns counts plus persisted audit failure markers. It is a request-time observation, not continuous audit coverage or enforcement; corruption refuses with a preserved-evidence message. Runtime resource cards add exact decimal-string peak RSS (kernel high-water) and dashboard-process user/system/start CPU ticks in raw kernel accounting units, explicitly not milliseconds, worker aggregates or hard OS enforcement.

Frozen campaigns expose `GET /api/campaigns/{id}/verify-inputs`, the read-only re-verification behind `corpus-campaign-verify`: the frozen plan file is re-read and hash-compared, and a changed build/trial scope refuses. Input rotation and rotation detail panels render exact owned-visit/member CPU-tick charts from the already-verified snapshot strings, keeping measured and unmeasured visits separate; charts are observations, not equal allocation.

Heavy operations share one bounded blocking-work slot and a60/minute rate bucket in addition to the existing global request limit; inventory verification is explicit, not part of the10-second refresh loop. Chromium acceptance (`scripts/browser-operations-check.cjs`) covers auth/CSRF/origin/body guards, exact exports without local paths, stale-confirmation refusal, activation/reuse, retained refusals, tampered chunks, corrupt decision history, exact coverage months/gaps, stale-response ownership, registry-audit corruption refusal, desktop/mobile and restart persistence. No acquisition, live trading, public exposure or HTTPS mode is introduced.
