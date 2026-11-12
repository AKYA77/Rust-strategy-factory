# Next action

Status: SOFTWARE_INCOMPLETE. Read AGENTS.md, PROJECT_STATE.json, ACCEPTANCE.md and IMPLEMENTATION_QUEUE.md. After every verified milestone update this file, reread it, and begin the next unfinished task.

## Latest verified milestone

64 application tests, formatting, Clippy and release build pass. Schema v3 supports an actual standalone Rust research loop: one bounded worker/task, fair input rotation, persisted expanding region/candidate cursors, expiring coordinator and experiment leases, immutable results, cache recovery, pause/resume/stop and state-file budget backpressure. A separate-process test verifies live controls and safe shutdown. The documented sample command completed ten steps: three new tick replays, seven zero-signal rejections, no failures; no process is left running. Results are SYNTHETIC_TEST_ONLY and training-only.

Conditional candle barrier bounds preserve SL/TP and entry/deadline ambiguity, including unknown bid/ask correlation after spread floors. Four thousand seeded bound queries contain the true sequential outcome. Tick results persist conditional bound audit counts; unclosed final bars remain uncovered. These are not net strategy win-rate bounds. Default screen still only safely rejects proven absence of signals; candidate-level performance bounds and rejected-candidate audit policies remain unfinished.

## Begin next

Build the authenticated localhost dashboard using the actual persistent backend. Add secure password initialization, sessions, CSRF/body/rate controls, run pause/resume/stop, experiment/strategy/trade detail, data provenance, truthful validation/leaderboard states and exports. Verify actual browser authentication/workflows on desktop/mobile, and service restart persistence. Then complete independent scientific workflow, HTF/operators and bounded improvements, native storage/import/source review, full screening policies, worker supervision/OS resource controls and deployment/end-to-end acceptance. Reconcile the full queue rather than stopping after the UI.

See RUNBOOKS/research-loop.md for the tested process and limits. All nine family templates and pending entry integration are implemented. Reviewed broker/DST profiles, representative real quotes, full history and VPS acceptance remain external pending work. Running network policy still excludes saved provider/API draft domains; hosted CI/source terms remain unverified. No qualified strategies or dashboard yet. Research does not keep running merely because Codex updates this file; only an explicitly launched Rust process can do that.
