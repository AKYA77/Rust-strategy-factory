# Next action

Read AGENTS.md, PROJECT_STATE.json and ACCEPTANCE.md. Continue implementing; do not treat this checkpoint as the requested finished product.

Registry/schema-v2, fenced leases, holdout reservation safeguards, numerical boundary checks and verified online backup are now implemented. Next: complete exact order lifecycle (pending market/stop/limit/cancel/expiry), event-aligned conversion provenance and reviewed profiles/calendars; expand all nine strategy families and conservative candle outcome bounds; then build the resource-bounded durable scheduler and real dashboard. Do not enable the experimental index until complete accounting differential tests and measured benefit pass. Also address the broader unfinished queue. Preserve synthetic versus historical evidence distinctions.

Activate cloud tools using the exports in AGENTS.md. Run scripts/check.sh before committing. Push verified milestones and confirm remote SHA. Do not touch hermes-factory. No production processes currently need startup.

Last complete local verification: 28 integration tests, formatting and Clippy passed. Synthetic benchmark saved with source/binary hashes; 3 workers fastest in that one short baseline. No real data acquired, no qualified strategies. GitHub API inspection is blocked by running network policy; api.github.com is saved as a draft addition. Review/save/publish settings to apply it. The previous verified remote checkpoint is 16d71c0; consult Git history for the second checkpoint. No code executes after the session ends unless a process is explicitly started; no scheduler exists yet.

## Latest order checkpoint

35 tests, formatting and Clippy pass. `orders` now implements pending market/stop-market/limit lifecycle, cancellation, expiry and serialized snapshots; `tick_engine` shares market-fill arithmetic. CSV archive hashing now covers the exact parser stream. Three alternating baseline/refactor benchmark pairs are saved; timings are noisy and do not establish a speedup.

Immediate implementation task: add typed pending entry intents to the strategy grammar and replay, with a documented choice of order placement versus fill-time sizing, risk/margin reservation, cancellations and receipts. Do not pretend the standalone order lifecycle already provides this integration. Continue with all nine families, candle outcome bounds, bounded durable scheduler, complete scientific safeguards and the dashboard. Real quote sources, approved broker/DST profiles and hosted CI inspection remain externally unverified. Do not declare software complete. The full queue remains in IMPLEMENTATION_QUEUE.md.
