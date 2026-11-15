# Fleet pause intent and state epochs

Schema18 gives each fleet a verified state head and bounded immutable transition chain. Pauses distinguish operator, scheduler idle and legacy unknown. Old paused fleets remain unknown with no invented pause time; migration preserves frozen definitions and revokes old coordinator nonces. Current-schema missing heads are refused rather than repaired.

Every meaningful state/intent transition increments an exact epoch. Operator pause on an already idle/unknown pause asserts operator intent; repeated identical operator controls are idempotent. Coordinator claims/fences now bind fleet epochs as well as nonces and attempts. Scheduler publication atomically records idle/exhausted state with the rotation prefix. A failed journal write rolls back state, epoch and nonce effects.

The `resume_idle` library primitive requires the same verified scheduler-idle epoch and complete transition detail. It never resumes operator, legacy or terminal state. It starts no worker and does not grant source retry admission. Detail is bounded128/fleet and2048/global; omitted counts remain explicit. Capacity may hold automated resume but cannot prevent operator stop. Dashboard pause remains available on a scheduler/unknown pause so operators can assert a durable hold.

Five new Rust tests cover actual idle/exhausted transitions, operator overrides/idempotence and backup, competing resume and operator races, migration/key preservation/current-head refusal, journal-cap stop safety and injected rollback/coordinator fencing. Ten fleet process regressions pass. Actual Chromium verifies scheduler-idle/operator distinction and exact epoch/history exports across existing controls, mobile and restart. Opt-in bounded automatic fleet wait/recovery remains next work.
