# Next action

Status: SOFTWARE_INCOMPLETE. Read AGENTS.md, PROJECT_STATE.json, ACCEPTANCE.md and IMPLEMENTATION_QUEUE.md. After each verified milestone update this file, reread it, and begin the next implementable task.

## Latest verified milestone

65 Rust application tests, formatting, Clippy and release compilation pass. Actual Chromium desktop/mobile acceptance passes against the Axum server and persisted SQLite fixtures: login/logout, Argon2 password file permissions/no overwrite, session flags, CSRF/origin/host rejection, body/rate limits, forbidden metrics writes, durable run controls, strategies/trades/equity, exact exports, numerical training diagnostics and restart persistence. Screenshots were inspected and are ignored under state/browser. Test processes are stopped.

The dashboard is a functional core, not the complete product. See RUNBOOKS/dashboard.md for initialization/start/test commands and current scope. There are no qualified strategies. Large i128 monetary receipts now serialize losslessly; the UI declines unsafe-range numeric display. The durable standalone loop, all nine base families and pending strategy order integration remain tested. See RUNBOOKS/research-loop.md.

## Begin next

Implement chronological execution windows with prior-only warmup and no pre-window positions; then complete out-of-sample/walk-forward evaluation, frozen candidate/evidence identities, corpus-wide protected holdouts, multiplicity/cost/neighborhood/regime checks and real qualification state transitions. Do not promote numerical training results to validated status. Continue through HTF/operator/search improvements, candidate-level net candle screening, native lossless storage and real source/import acceptance, diagnostics, worker supervision/OS resource controls, production deployment and full end-to-end acceptance.

Remaining dashboard operations include full pagination, data import/approval/refresh, backup/restore interfaces, worker management, richer charts and complete validation/leaderboard workflows. Current controls update desired state but do not start a missing coordinator. Local HTTP only; no public exposure/HTTPS proxy mode is implemented. No live dashboard or research process is left running by tests.

Representative real quotes, broker/DST specifications, full history and target-VPS acceptance remain pending. Running network policy still excludes saved API/provider domains, so hosted CI/source terms remain unverified. New environment setup/start instructions are draft configuration until reviewed/saved/published. Continue independent implementation; do not stop merely because this milestone passed.
