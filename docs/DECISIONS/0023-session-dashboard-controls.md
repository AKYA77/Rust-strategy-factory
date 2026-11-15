# Authenticated foreground session observations

The dashboard exposes bounded verified inventory and exact detail/status exports at `/api/supervision-sessions` and `/{id}`. POST `/{id}/stop` accepts only an empty object, validates the session identity and uses the service clock. Existing authentication, origin, CSRF and body limits apply. No executable, PID or caller-supplied timestamp is accepted. Finished/already-requested stops are idempotent. The route starts no processes.

Session snapshots use request ownership: selecting another record invalidates old responses and exports immediately. Stop requests invalidate the displayed snapshot before posting, then revalidate. Finished and already-requested records disable stop. Body/receipt corruption disables verification, controls and exports. No PID-based kill or liveness interpretation exists.

The actual Chromium/Axum/SQLite search suite covers authentication, missing/bad IDs, CSRF/origin, rejection of caller PID fields, terminal idempotence, exact string fields/raw downloads, two actual CLI session records, delayed response ownership, mobile width, restart parity and corrupted body refusal. Cooperative live stop and abrupt-death behavior also have actual Rust process evidence from the preceding session milestone.
