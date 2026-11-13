# Local dataset observation catalog

```sh
rust-strategy-factory catalog-record dataset.rsf catalog.sqlite
rust-strategy-factory catalog-list catalog.sqlite
rust-strategy-factory catalog-list catalog.sqlite - 25
rust-strategy-factory catalog-entry catalog.sqlite DATASET_IDENTITY
```

Registration accepts the existing normalized JSON/native/RSFZ/Parquet loaders. It verifies the dataset, records normalized content/provenance/profile identity and recomputes observed ties, repeated quotes and gaps over one hour from actual events. Exact first/latest sequence values use decimal strings. The separate catalog contains metadata, not quote archives or artifact locations. It observes the supplied dataset at registration time; it cannot establish that an archive remains present, unchanged or available afterward.

An immutable receipt records local registration time, independently of the input's import timestamp. Reimporting identical content/provenance with a different import time reuses the original receipt/hash. Different declared source/archive/profile/parser/category identities remain separate even when their normalized quote content is identical. This inventories distinct provenance revisions; correction-parent edges and source revision approval workflows remain unfinished.

`declared_class` is copied metadata, not catalog approval. Receipt and page limitations explicitly deny provider authenticity, acquisition availability, retention, calendar/quality/profile approval, complete history and research eligibility. Registration never changes the input class or qualifies a strategy. Qualified leaderboards remain empty. Catalog metadata must not substitute for independent approval evidence.

SQLite WAL/immediate transactions serialize admission. Application identity refuses unrelated databases; existing/list/detail mode opens read-only and never creates a missing file. Managed rows are append-only with update/delete refusal triggers and verified canonical receipt hashes. Requested corrupt/conflicting receipts refuse rather than being repaired or overwritten. Local SQLite protections and hashes are not authentication against an operator who can rewrite the entire database.

Metadata JSON is stored as bounded BLOBs, allowing SQLite byte-length checks before returning payloads to Rust. Limits:2million events observed per registration;128KiB per stored receipt;10000 entries/128MiB logical JSON payload;1..100 entries per page. Source/profile metadata and decimal sequences are validated. Logical quotas exclude SQLite/WAL/filesystem overhead. Full production retention/backups/resource enforcement and target-VPS scale acceptance remain open; existing experiment-registry backup commands do not automatically back up this separate catalog.

Pages order full hex provenance identities, not chronological event time. Resume with `next_cursor`; use `-` for the first CLI page. Concurrent inserts before an already-used cursor appear on a fresh traversal; existing immutable entries do not move. No missing-range/calendar or source completeness inference is made from the catalog.

Tests cover actual CLI record/list/detail/reuse, reimport stability, separate archive revision identities, independently recomputed counters, full-u64 boundaries, cursor insert stability/bounds, invalid data/metadata, requested receipt tampering refusal, immutable SQL rows, unrelated/missing database preservation and a10000-row explicit metadata-only quota fixture. These are functional fixtures, not real quote or production performance acceptance. Dashboard import/catalog/approval integration and source acquisition remain unfinished.
