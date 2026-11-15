# Retry policy operator workflows

The bounded CLI accepts `corpus-retry-configure DB SEARCH_ID POLICY.json` and `corpus-retry-admit DB SEARCH_ID EXPECTED_EPOCH`. Policy files share the actual-read1 MiB operator limit and reject unknown fields. Epochs use exact decimal parsing. Commands return self-attributed policy/decision JSON and launch no workers. Existing source status/export includes the frozen policy and policy hashes on automatic admission provenance.

Authenticated POST retry-policy/retry-admit routes enforce existing session, CSRF, origin and body guards. HTTP uses the server clock; request bodies cannot supply a timestamp. Epochs are decimal strings. Policy retuning conflicts, terminal source controls remain irreversible, and hold/backoff are explicit successful observations rather than fictitious completed work.

Dashboard cards offer bounded visible policy inputs, an explicit immutable freeze action, and an epoch-fenced admission check only for configured worker-error pauses. Raw action downloads retain the exact backend response with source identity. Request ownership prevents delayed older action responses from replacing the current decision/export. No endpoint starts arbitrary programs or accepts performance metrics.

An actual CLI test covers frozen/idempotent configuration, durable backoff across processes, invalid epochs, retuning refusal and operator-pause preservation. Actual Chromium covers security/refusal, immutable controls, permanent-error hold, byte-exact downloads, delayed-response ownership and desktop/mobile/restart. This is an operator admission workflow, not an enabled background process supervisor.
