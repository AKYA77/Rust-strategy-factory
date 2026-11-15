# Durable foreground invocation observations

Schema15 records an unpredictable session identity and private owner token after resource admission and search/policy verification. Heartbeats carry actual rounds, checks and monotonic waited milliseconds within command budgets. Public status exports exact decimal strings, source identity, actual PID metadata and timestamps; it never exports the token. PID metadata is not a process handle or a liveness claim.

Owner-fenced completion atomically clears the token and writes an immutable hash-verified terminal receipt. Receipt failure rolls back completion. Operator stop requests are idempotent, preserve the first observed timestamp and are polled at safe round/wait boundaries. They neither kill a PID nor change search intent. At most128 sessions are retained; a full inventory refuses new sessions but permits existing stop requests/completion. No evidence is silently deleted.

Unclosed sessions remain unclosed after SIGKILL or errors; restart creates a distinct session and does not manufacture a terminal outcome or extend the retry deadline. Registry integrity checks validate body/identity/counter bounds and terminal receipt consistency. Verified backup preserves sessions, stop requests and receipts. Schema migration invalidates old coordinator leases as with prior migrations.

Evidence includes owner/counter/terminal fences, injected receipt-write rollback, inventory cap with stop/finish still available, backup parity, actual cooperative stop, actual abrupt process death/reaping and restart identity/deadline preservation. Fixtures are synthetic and development fault metadata; no historical failures or scientific qualification are claimed. Owned child-process supervision and dashboard session controls remain next work.
