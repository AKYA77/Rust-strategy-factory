# Atomic bounded retry decision observations

Schema14 adds immutable hash-chained retry observations and a verified append head per search. Admission records held, backoff and requeued outcomes in the same transaction as any queue/control change. If journal insertion fails, queue changes, control epoch and prior-error records roll back together. Responses include the stored sequence/hash. Head count/hash prevents a deleted suffix from appearing complete.

Tracking starts at an observed source state epoch; old decisions are never fabricated. Each source retains at most128 observations; global limits are4096/8 MiB with conservative4096-byte row headroom. At capacity, admission returns an explicit decision-history-full hold without new mutation. Operator pause/stop uses independent control metadata and remains available. No automatic pruning deletes observations.

Status includes a bounded recent summary. CLI, authenticated API and dashboard history paging expose every retained observation, with decimal-string counters/times and source/immutable-row-bound cursors that remain valid across appends. Raw page downloads preserve backend bytes. Corruption, wrong-scope or wrong-record cursors refuse verified history. Counts denote observations, not worker starts, simulations or qualifications.

Tests cover atomic rollback, immutability/suffix refusal, backup independence, cap/stop behavior and append-stable cursor traversal. Actual Chromium pages beyond the recent-summary limit and exports exact history on the real service. No foreground/background worker is launched by history or admission views. Owned process and foreground session lifecycle remain next work.
