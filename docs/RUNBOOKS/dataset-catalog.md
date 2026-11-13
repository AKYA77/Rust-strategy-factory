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

Metadata JSON is stored as bounded BLOBs, allowing SQLite byte-length checks before returning payloads to Rust. Limits:2million events observed per registration;128KiB per stored receipt;10000 entries/128MiB logical JSON payload;1..100 entries per page. Source/profile metadata and decimal sequences are validated. Logical quotas exclude SQLite/WAL/filesystem overhead. Verified standalone catalog backup/new-path restore now exist as described below. Full production retention/resource enforcement, coordinated multi-component backups and target-VPS acceptance remain open; existing experiment-registry backup commands do not automatically include this catalog.

Pages order full hex provenance identities, not chronological event time. Resume with `next_cursor`; use `-` for the first CLI page. Concurrent inserts before an already-used cursor appear on a fresh traversal; existing immutable entries do not move. No missing-range/calendar or source completeness inference is made from the catalog.

Tests cover actual CLI record/list/detail/reuse, reimport stability, separate archive revision identities, independently recomputed counters, full-u64 boundaries, cursor insert stability/bounds, invalid data/metadata, requested receipt tampering refusal, immutable SQL rows, unrelated/missing database preservation and a10000-row explicit metadata-only quota fixture. These are functional fixtures, not real quote or production performance acceptance. Dashboard import/catalog/approval integration and source acquisition remain unfinished.

## Verified snapshot and new-path restore

```sh
rust-strategy-factory catalog-backup catalog.sqlite new-snapshot.sqlite
rust-strategy-factory catalog-restore new-snapshot.sqlite new-catalog.sqlite
```

Both commands require a new destination. A consistent SQLite read transaction includes committed WAL rows while verifying source integrity, managed schema and quotas. Each receipt/hash is checked and copied unchanged into a fresh guarded catalog; row counts and destination SQLite integrity are checked before publication. The private staged database is checkpointed and switched to a standalone journal mode, closed/synced and hard-linked without replacement, then the directory is synced. Only this operation's random temporary files are cleaned up. Corrupt receipts, unknown table/column state or failed validation cannot publish a partial destination. The source snapshot has a512MiB page budget in addition to the logical catalog quotas.

The snapshot contains catalog metadata only, not the experiment registry, raw archives, import ledger/chunks or normalized artifacts. Back up those separately. Restore creates another verified catalog and never activates it over an existing database. No data approval is added. Tests cover live committed WAL, exact hashes, later source appends, normal writes after restore, no overwrite, refusal/cleanup for corrupt or unknown state, preservation of unrelated files and actual CLI backup/restore. Comprehensive concurrent-write/crash/disk-full/power-loss/production recovery remains separate; no speed or full-scale storage acceptance is claimed.

## Authenticated dashboard browsing

```sh
rust-strategy-factory serve registry.sqlite auth.json 8080 catalog.sqlite
```

The catalog argument is optional; the existing three-argument `serve` workflow remains supported. A supplied catalog must already exist and pass a verified initial read; missing/unrelated catalogs or a corrupt initial read refuse startup. Subsequent requested corrupt receipts fail closed. The server opens it read-only for authenticated `/api/catalog` pages and `/api/catalog/{identity}` detail. Request parameters cannot select filesystem paths. No registration/approval/restore write endpoint is exposed.

The Data panel distinguishes an unconfigured catalog from verified observations and from an unavailable/corrupt catalog. Identity cursor navigation shows bounded pages and explicit declared classification/source/observed time/sequence limits. Inspecting an observation brings its receipt into view. Exact decimal sequence strings remain intact, generic unsafe-number guards protect previews, and exported JSON preserves the server's full exact receipt. Failed catalog reads clear displayed observations/selection instead of reporting a verified empty inventory. The existing experiment-page provenance panel stays separate.

Actual Chromium acceptance covers legacy unconfigured startup, configured restart, authenticated query bounds/read-only refusal,31 observations across pages, desktop/mobile full-u64 detail/export, source/classification limits and corrupt-state500/refusal. A mobile regression first detected long identity-label overflow; wrapping now passes, with screenshots under ignored `state/browser/catalog-1280.png` and `catalog-390.png`. Import/registration/approval and backup activation dashboard workflows remain unfinished. The service still binds loopback; optional catalog browsing does not implement public HTTPS mode.

## Immutable correction lineage

`catalog-correction <dataset> <catalog.sqlite> <parent-identity> <evidence-reason>` registers a new observation as a correction of an already registered observation. Register the correction directly: an existing unlinked observation cannot later be rewritten to add a parent. Matching repeated requests reuse the exact receipt; changed reasons/parents conflict. The parent must have the same declared source, instrument, classification and observed time boundaries. Profile/content/archive/parser revisions can differ; none constitutes approval. A bounded nonempty reason records the operator's evidence reference, not independently verified provider truth.

Corrected receipts use v2; unchanged legacy observations keep v1 serialization and hashes. Reads, pages and snapshots validate ancestry, missing parents, cycles and scope, with a maximum of64 observed nodes. New-path snapshots/restores retain the original parent receipts and child hashes. An older reader safely refuses corrected receipts it cannot interpret. Corrections never erase original evidence, change declared class, approve data or invalidate protected scientific reservations. Approval/review decisions and range-extension/split/merge lineage remain separate unfinished workflows.
