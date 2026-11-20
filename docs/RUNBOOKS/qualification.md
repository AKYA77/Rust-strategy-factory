# Qualification software runbook

Independent strategy qualification: immutable decisions, evidence/approval gates and eight
diverse top-five leaderboards (blueprint §8/§20, DECISIONS/0057). Historical research and
paper simulation only. **Synthetic or diagnostic evidence can never qualify.** Zero
strategies are qualified until complete approved real evidence exists.

## Concepts

- `qualification::Policy` — frozen versioned contract. `Policy::owner_selected()` pins the
  blueprint floors and the owner-selected 22-month window (2025-01..2026-10). Weaker
  policies are refused; stricter ones are allowed.
- `qualification::EvidenceBundle` — one strategy on one instrument: canonical strategy,
  declared data class, dataset/profile/cost identities, execution versions, five-area data
  approval snapshot, complete coverage receipt, full-period registered accounting, frozen
  scientific protocol evidence.
- `qualification::assess` — ten fail-closed gates. States: `REJECTED` (contract violation,
  synthetic/diagnostic, failed numerical floors), `HELD` (missing evidence/approvals),
  `QUALIFICATION_EVIDENCE_COMPLETE` (all gates pass). The assessment never grants
  qualification itself.
- `qualification::Store` — separate SQLite WAL journal. Append-only decisions
  (Qualify/Reject/Revoke) with keyed authenticators, expected-head fencing and quotas;
  immutable per-instrument board snapshots published atomically with each decision.
- Boards — at most five authenticated qualified strategies per instrument, ranked by
  `diverse-ranking-v1` exact-integer scores over verified evidence only, with at most two
  strategies per structural diversity class. Empty boards carry accurate reasons; excluded
  candidates carry exact reasons; rejected/revoked history is retained.

## CLI

```
qualification-policy
    Print the owner-selected frozen policy JSON.

qualification-assess <bundle.json> <policy.json>
    Pure gate assessment (no writes). Prints the assessment JSON.

qualification-decide <store.sqlite> <authority.secret> <reviewer> <request.json> \
                     <bundle.json> <policy.json> [registry.sqlite catalog.sqlite archive.csv dataset.json]
    (password from stdin) Append an immutable decision. request.json:
      {"decision":"Qualify|Reject|Revoke","expected_head":null|"<64-hex>",
       "submitter":"<researcher>","explanation":"...","evidence_text":"..."}
    Qualify REQUIRES the four live-artifact arguments and complete gates; the store
    revalidates the registry experiment, frozen protocol report and catalog eligibility
    at decision time. Reject/Revoke may omit them (pass nothing). expected_head must be
    the current scope head (null for a fresh scope; read it from qualification-scope).

qualification-board <store.sqlite> <authority.secret> [instrument]
    Current authenticated board(s): one instrument or all eight. Entries, excluded
    candidates with reasons, empty reasons, policy hash and journal head.

qualification-scope <store.sqlite> <authority.secret> <instrument> <concept-id>
    Full decision chain for one strategy scope and its current state
    (QUALIFIED/REJECTED/REVOKED/UNDECIDED) under the current authority.

qualification-history <store.sqlite> <filter.json>
    Archive of decisions incl. rejected candidates. filter.json (all fields optional):
      {"instrument":"EURUSD","concept_id":"<64-hex>","decision":"Qualify|Reject|Revoke",
       "diversity_class":"<label>","timeframe":"M1","from_ms":0,"to_ms":0,
       "after_sequence":0,"limit":50}

qualification-leaders <store.sqlite> <instrument> [after-sequence-or-- limit]
    Retained immutable board snapshots (previous leaders), keyset paginated (1..200).

qualification-backup <store.sqlite> <new-backup.sqlite>
    Verified no-overwrite snapshot; restore by opening the copy with qualification-board.
```

## Authority provisioning

Reuse `review-authority-init` with grants that include the `ScientificQualification` area.
Grants scope symbol + source + submitter; the reviewer must differ from the submitter and
the grant must be unexpired. Example reviewer grant:

```json
{"name":"independent-quant-reviewer","password_hash":"<argon2id>",
 "grants":[{"symbol":"EURUSD","source":"<dataset source>","submitter":"<researcher>",
            "areas":["ScientificQualification"],"expires_ms":1800000000000}]}
```

The five data-review areas (Source/Profile/Calendar/Quality/Coverage) are approved through
the existing `data-review-decide` flow; qualification consumes the resulting
`data-eligibility` state and revalidates it live at decision time.

## Operational boundaries

- A Qualify decision is refused unless every gate passes AND live revalidation succeeds.
  Revoking a data approval, changing archive bytes, or rotating the authority invalidates
  board eligibility; history is never rewritten.
- Board reads authenticate the latest decision per scope under the CURRENT authority.
  Expired grants or rotated authorities exclude entries with explicit reasons; operators
  revalidate and re-decide, the software never auto-qualifies or auto-revokes.
- The qualification store is independent of the experiment registry; back it up with
  `qualification-backup` alongside registry/catalog snapshots.
- Dashboard leaderboard views are a separate dashboard-lane deliverable consuming this
  library/CLI contract.
