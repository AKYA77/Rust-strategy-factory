# Data acquisition and provenance state

No genuine quote archives have been acquired or accepted. All executable fixtures/benchmarks are SYNTHETIC_TEST_ONLY. Source terms, licensing, representative real bid/ask formats and 2025/2026 availability still require review. No actual historical strategy performance is claimed.

Source discovery probes on 2026-11-12:

- `https://www.histdata.com/download-free-forex-data/`: inaccessible under the running cloud network policy; a subsequent probe reported proxy CONNECT 403. No page, terms or archive was retrieved.
- `https://www.dukascopy.com/swiss/english/marketwatch/historical/`: HTTP request failed with 403 under the same policy. Provider availability and terms are unverified.
- GitHub hosted CI API at `https://api.github.com`: proxy CONNECT 403. Native Git read/push to the selected repository works.

Environment draft custom domain additions: api.github.com, www.histdata.com, www.dukascopy.com, datafeed.dukascopy.com. These permit future source/terms discovery and potential sample acquisition after review/save/publish; saving the draft does not apply runtime access. Runtime status still reported an enforced policy with no custom hosts. Do not interpret domain configuration as permission to bypass provider restrictions or as source acceptance. Retry fresh requests after the policy changes, inspect actual terms/formats, then select short windows for all eight instruments and both years within the approximately 1 GB compressed sample budget. The full backfill remains for the VPS.

The normalized CSV importer records source description, import timestamp, archive BLAKE3, parser version, content/profile hashes, exact event coverage, ties, repeated quote events and coarse gap counts. Source licensing/acquisition timestamps, calendar-aware gaps, source revisions, resumable native downloads remain unfinished. Bounded Parquet/ZSTD storage now exists; see ACCEPTANCE.md for its scope and source-pinned synthetic comparison. Import time is not falsely labeled acquisition time. Parse success can promote only to REAL_STRUCTURALLY_VALID, and cannot establish research eligibility or profile approval.

## Resumed source access check (2026-11-13)

Fresh source-info requests to `https://www.dukascopy.com/swiss/english/marketwatch/historical/` and `https://www.histdata.com/download-free-forex-historical-data/` each returned HTTP403. No terms/content were obtained; no licensing/availability conclusion and no real quote acquisition claimed. Runtime restricted policy remains enforced with no custom source domains. Proposed additive domains: `www.dukascopy.com`, `datafeed.dukascopy.com`, `www.histdata.com`, `histdata.com`, preserving existing rules/presets. Draft saving returned `draft_not_editable`; changes are NOT confirmed saved/applied. The proposal is retained for settings transfer, and independent engineering continues without repeated same-state requests. Once access is updated, refresh observed policy and retry a fresh source/terms request before any small permitted sample download. Complete two-year backfill remains deferred to the VPS.

## Bounded observed coverage inspection

`coverage <dataset> <start-ms> <end-exclusive-ms> <gap-threshold-ms>` reads and verifies an existing normalized JSON/native/RSFZ/Parquet artifact without downloading it. The read-only report binds provenance identity and the original manifest/class. It lists exact scoped first/latest event timestamps and decimal sequence strings, each intersected UTC month including empty and partial months, and the total adjacent in-scope gaps strictly greater than the positive threshold. At most1000 gap details,120 intersected months and2million input events are accepted; gap totals remain exact when detail output is truncated. Scope is half-open; tied timestamps retain source sequence order. Events outside the request cannot create an internal gap.

This is observed event inventory, not a completeness certificate, missing-range download plan, source revision catalog, acquisition checkpoint or quality/profile approval. Empty months and long elapsed gaps may reflect closures or absence; an approved trading calendar and source inventory are still required to distinguish them. No quotes are fabricated and no classification is promoted. Original source/archive bytes are untouched. The existing bounded loaders still materialize the artifact; this is not full-history streaming.

Bounded local checkpointed normalized import now exists: see RUNBOOKS/normalized-import.md for source/revision binding, no-overwrite publication, exact recovery and explicit limits. This is not resumable provider acquisition or full-history streaming. Completed bounded jobs can now export one continuous native dataset with verified source/category and recomputed cross-boundary diagnostics; revision/catalog/calendar-aware completeness and automatic streaming catalog loading remain pending.
