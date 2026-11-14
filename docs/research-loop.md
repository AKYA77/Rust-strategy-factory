
### Registered synthetic corpus recovery

`corpus-step SPEC_JSON STRATEGY_JSON REGISTRY MAX_CANDLES FEATURE_CELLS MAX_RECEIPTS NEW_CHUNKS LEASE_MS` executes a bounded step against a frozen registered identity. `NEW_CHUNKS` is 1–64 and the lease is 1–86400000 milliseconds. Repeated invocations resume committed account checkpoints, yield pending leases, and reuse verified completed receipts. A failed terminal result stays failed. The current implementation revalidates source artifacts and recomputes the saved prefix before advancing; it provides recovery semantics without a fast-seek throughput claim.

Use `corpus-status REGISTRY EXPERIMENT_ID` to inspect verified cursor/control metadata. `corpus-control REGISTRY EXPERIMENT_ID paused` prevents new claims and final publication; `running` resumes a paused job. `stopped` is irreversible for that identity. Existing in-flight work remains subject to lease and publication fencing. A registry backup preserves checkpoints and controls, but does not package the source corpus artifacts: restore those verified artifacts separately. Recovery tests cover a killed process waiting for a database writer and independent continuation of a paused backup. Power-loss and disk-full acceptance remain unfinished.

This path accepts synthetic training corpora with literal USD conversion only. Real, protected and unsupported conversion contexts are refused. Synthetic receipts cannot qualify a strategy.
