# Next implementation: durable generated corpus backlog

Status: DESIGN_NOT_IMPLEMENTED. Existing corpus sessions execute a supplied finite strategy list; the materialized research Driver remains a separate one-worker search coordinator. The corpus pool does not yet replenish its own candidate backlog.

Reuse research::candidate_at(index, seed) and the authoritative corpus step/batch path. Freeze corpus/source identity, strategy-generator method, seed, costs, execution budgets and compute revision before admitting jobs. Preserve synthetic-training/USD refusal; no real/protected or independent evidence may feed search. Operator worker counts and admission-pressure thresholds may vary without changing financial inputs.

Required implementation boundaries:

- Persist a bounded generated-job queue with immutable candidate-index/family/strategy/experiment-key links and explicit pending/completed/terminal/error states. Canonical duplicate strategies may share a verified experiment receipt, but never masquerade as new tick executions. Invalid grammar slots need durable bounded reasons and monotone generation progress rather than endless regeneration.
- Use a token/attempt/expiry-fenced coordinator distinct from child experiment leases. Admission and status/cursor updates must be transactional. A replaced coordinator cannot publish queue progress; already committed child receipts remain recoverable through frozen keys. Reuse/extract the existing research lease semantics where practical rather than weakening either coordinator.
- Bound queue depth, candidate generation per step, active workers, concurrent object budgets and persistent metadata. Apply resource admission checks before rounds. Yield at safe boundaries and preserve pause/resume/irreversible-stop controls. Do not sleep-spin on held jobs or failed evidence.
- Recover an admitted partial batch before replenishing it. Commit cursor/counters only with durable queue links, count candidate slots separately from actual immutable simulations, and preserve exact restart positions. Exhausting one eligible region should replenish the next bounded deterministic region, not silently restart old regions.
- Test stale-coordinator races, partial child-result publication, actual process interruption/restart, duplicate keys, generation/execution errors, controls, quotas and receipt parity with scalar corpus replay. Browser status must distinguish this search coordinator from a static operator batch and from the older materialized Driver.

Current verified primitives: Registry experiment/checkpoint fencing and backups; corpus resume/yield/result reuse; bounded parallel rounds; deterministic candidate_at; Linux round-boundary admission checks. Remaining work is executable queue/coordinator integration and acceptance, not a claim of completed autonomous multiworker research.
