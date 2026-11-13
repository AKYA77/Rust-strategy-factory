# Checkpointed local normalized imports

This command reads an existing local CSV; it never downloads quotes or promotes scientific approval. The existing one-artifact `import` command remains available. Use a dedicated directory for each exact source revision and import configuration:

```sh
rust-strategy-factory import-step existing.csv profile.json 'source URL and provenance description' import-job real 50000
rust-strategy-factory import-status import-job
```

Repeat the same `import-step` invocation until `complete` is true. Each invocation publishes at most one immutable native chunk. A subsequent invocation of a completed job revalidates the source and committed chunks, without republishing. `import-status` verifies committed artifacts without requiring the original CSV; it cannot certify that the original archive still exists. Preserve the original source independently. No raw archive copy is created by this command.

The normalized header is exactly `timestamp_ms,sequence,bid,ask`. Prices use the explicit profile's exact grid; events retain source order and full-u64 sequences. Tied timestamps are allowed only with increasing sequence. CSV quoting/CRLF and files without a final newline are supported. A gap is never filled or sorted away. Invalid prices, timestamps, crossed quotes, duplicate/out-of-order sequences and malformed rows refuse the current chunk. Earlier committed chunks remain, the failed checkpoint does not advance, and the job remains incomplete. No invalid partial artifact is published or silently skipped. This is refusal with retained progress, not a persistent row-quarantine browser workflow.

## Identity and publication

The entire bounded source byte snapshot is hashed on every invocation. Exact source bytes, parser version, profile, source description, category and chunk size are bound to the job. A changed archive/configuration refuses; use a new directory for a revision. Moving an identical archive to a different input path is allowed. Synthetic input must use `synthetic`; it remains `SYNTHETIC_TEST_ONLY`. Successful `real` parsing assigns at most `REAL_STRUCTURALLY_VALID`, never provider authenticity, calendar/quality/profile approval or research eligibility. All tests and measurements are synthetic fixtures, including the test that exercises this real-parser classification ceiling.

A separate `normalized-import.sqlite` WAL ledger stores a hashed checkpoint, source/configuration, stable import timestamp, exact byte cursor and ordered chunk identities. It does not change the experiment registry schema. An application identifier rejects unrelated databases. An immediate transaction serializes chunk work; a competing writer may return a busy refusal and can retry. The native chunk is checked/synced and published without replacement before checkpoint commit. After interruption, a matching already-published orphan is verified and reused exactly. A conflicting orphan is preserved and refused for investigation. Missing/damaged committed chunks and invalid checkpoints also refuse. Do not delete evidence to force progress.

Chunks share the full consumed archive hash and use `normalized-csv-chunks-v1` parser identity. Each contains its own exact normalized event subset. Chunk dataset identities deliberately differ from a monolithic import. Concatenated ordered quotes match the one-shot parser in tests/measurement; this is not equality of whole artifact identities. Source boundaries, cross-chunk ordering and completed row counts are preserved in the ledger. Existing research loaders accept an individual verified chunk, but automatic multi-chunk continuous replay/catalog loading is unfinished; an individual chunk does not establish whole-period coverage.

## Resource and acceptance limits

Source snapshot at most64MiB;1..100000 events per chunk;2million events/4096 chunks per job;128MiB total native artifact quota; bounded profile/source/checkpoint metadata. The ledger/temp/orphan files are not a filesystem-wide quota. Every step rehashes the source and validates all committed artifacts. This favors explicit bounded recovery over throughput and is not a full-history streaming importer, source revision catalog, resumable network acquisition or missing-range download planner. Approved calendars/source inventories, complete historical coverage, archive retention/quotas and dataset approval remain pending.

Tests cover separate process resume, combined whole-source quote equality/full-u64 ties, changed revision/configuration refusal, malformed/crossed/boundary-order failure without advancement, simulated publication-before-checkpoint rollback and exact orphan reuse/conflict preservation, missing/damaged artifacts, checkpoint corruption, unrelated ledger preservation, competing-writer refusal, source/chunk bounds, empty input, CRLF/quotes/blank tails/no final newline and classification ceiling. The rollback test models the crash window; actual SIGKILL/power-loss/disk-full/production-filesystem acceptance remains separate.
