# Storage report — VPS disk pressure

Measured 2026-11-16 on `/dev/sda1` (96 GiB):

- **Used: 92 GiB (96%); free ≈ 4.8 GiB.** Inodes 4% used (not a constraint).

## Major relevant directories

| Path | Size | Disposition |
|---|---|---|
| `/root/workspace/Rust-strategy-factory/target` | 14 GiB | Build cache (debug+release). Reusable; keep. |
| `/root/.hermes/cache/scratch/rsf-review` | 13 GiB | Reviewer's throwaway repo copy (includes its own `target/`). **Disposable.** |
| `/root/.hermes/cache/scratch/legacy-probe` | 1.9 GiB | Reviewer's throwaway probe crate. **Disposable.** |
| `/root/.hermes/cache/scratch/rsf-historical-*` | ~0.9 GiB | Test temp dirs (derived). **Disposable.** |
| `/root/workspace/historical-migration-output/full-100k` | 13 GiB | Verified corpus-admissible output. **Keep.** |
| `/root/workspace/historical-migration-output/full` | 13 GiB | Superseded 500k output (inadmissible). Reproducible from source. |
| `/var/lib/hermes-factory` | 14 GiB | Original archives + canonical Parquet + receipts. **Must keep.** |

## Required before more large work

Reclaiming the disposable scratch (~15.8 GiB) is sufficient for the next milestones and
does not touch source data, verified output or rollback material. The superseded `full/`
output is reproducible from `/var/lib/hermes-factory` and could be reclaimed later, but its
deletion is not required now and is not attempted.

**Owner approval required.** A previous cleanup attempt was not consented to and was not
retried. Nothing has been deleted. Estimated space required for the next milestone (real-data
pipeline + integration analysis) is well under 1 GiB; no new full-corpus allocation is
requested.

## Do-not-delete list (per assignment)

Original HistData archives, canonical corrected Parquet, provenance manifests, import
receipts, data-quality evidence, backups/rollback material, and verified native data without
a safe alternative recovery copy.
