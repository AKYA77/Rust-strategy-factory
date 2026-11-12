# Data acquisition and provenance state

No genuine quote archives have been acquired or accepted. All executable fixtures/benchmarks are SYNTHETIC_TEST_ONLY. Source terms, licensing, representative real bid/ask formats and 2025/2026 availability still require review. No actual historical strategy performance is claimed.

Source discovery probes on 2026-11-12:

- `https://www.histdata.com/download-free-forex-data/`: inaccessible under the running cloud network policy; a subsequent probe reported proxy CONNECT 403. No page, terms or archive was retrieved.
- `https://www.dukascopy.com/swiss/english/marketwatch/historical/`: HTTP request failed with 403 under the same policy. Provider availability and terms are unverified.
- GitHub hosted CI API at `https://api.github.com`: proxy CONNECT 403. Native Git read/push to the selected repository works.

Environment draft custom domain additions: api.github.com, www.histdata.com, www.dukascopy.com, datafeed.dukascopy.com. These permit future source/terms discovery and potential sample acquisition after review/save/publish; saving the draft does not apply runtime access. Runtime status still reported an enforced policy with no custom hosts. Do not interpret domain configuration as permission to bypass provider restrictions or as source acceptance. Retry fresh requests after the policy changes, inspect actual terms/formats, then select short windows for all eight instruments and both years within the approximately 1 GB compressed sample budget. The full backfill remains for the VPS.

The normalized CSV importer records source description, import timestamp, archive BLAKE3, parser version, content/profile hashes, exact event coverage, ties, repeated quote events and coarse gap counts. Source licensing/acquisition timestamps, calendar-aware gaps, source revisions, resumable native downloads and Parquet cold storage are still unfinished. Import time is not falsely labeled acquisition time. Parse success can promote only to REAL_STRUCTURALLY_VALID, and cannot establish research eligibility or profile approval.
