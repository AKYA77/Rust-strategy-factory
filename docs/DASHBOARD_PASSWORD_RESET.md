# Dashboard Password Reset — Owner-Controlled Procedure

## Preconditions (verified on this host)

| Item | Value |
|------|-------|
| Service type | System-level units (`/etc/systemd/system/`) |
| Dashboard unit | `rust-strategy-factory-dashboard.service` |
| Research unit | `rust-strategy-factory-research.service` |
| Installed release binary | `/root/.local/share/rust-strategy-factory/current/rust-strategy-factory` (symlink → `releases/4d8ecc88ad78960f2d2e6dacee9803a14b0a1773-head/rust-strategy-factory`) |
| Existing auth file | `/root/.local/share/rust-strategy-factory/config/admin.json` (mode 0600, 129 bytes) |
| Dashboard listener | `127.0.0.1:3000` (loopback-only, enforced by systemd `ProtectSystem=strict` and binary bind address) |

## Backup (preserve original before any change)

Execute on the VPS terminal:

```bash
cp -p /root/.local/share/rust-strategy-factory/config/admin.json \
      /root/.local/share/rust-strategy-factory/config/admin.json.bak-$(date +%Y%m%dT%H%M%SZ)
```

This creates a private, read-protected backup at a specifically identified path. The original `admin.json` is **not** deleted or modified at this step.

## Create replacement auth file (password entered privately at prompt)

The `auth-init` subcommand uses `create_new(true)` — it **fails if the target file already exists**. Therefore the new auth file is created at a temporary path first, then atomically swapped into place.

Execute on the VPS terminal:

```bash
/root/.local/share/rust-strategy-factory/current/rust-strategy-factory \
  auth-init /root/.local/share/rust-strategy-factory/config/admin.json.new
```

When prompted, enter the new password (16–1024 bytes). **Do not paste the password in chat.** The input is read from stdin; use your terminal's hidden input (echo disabled) if available.

The command writes an Argon2id hash to `admin.json.new` with mode 0600. It does not touch the existing `admin.json`.

## Verify the replacement file before activation

```bash
ls -la /root/.local/share/rust-strategy-factory/config/admin.json.new
```

Expected: mode 0600, valid JSON with `version` and `password_hash` fields.

## Activate (atomic swap, original preserved as backup)

```bash
mv /root/.local/share/rust-strategy-factory/config/admin.json.new \
   /root/.local/share/rust-strategy-factory/config/admin.json
```

The original `admin.json` remains available at the `.bak-*` path created above.

## Restart dashboard service

```bash
systemctl restart rust-strategy-factory-dashboard.service
```

## Rollback procedure

If activation fails or the new password is not accepted:

```bash
systemctl stop rust-strategy-factory-dashboard.service
cp -p /root/.local/share/rust-strategy-factory/config/admin.json.bak-* \
      /root/.local/share/rust-strategy-factory/config/admin.json
systemctl start rust-strategy-factory-dashboard.service
```

The dashboard reverts to the original authentication file. The `.bak-*` backup is preserved throughout.

## Post-authentication verification (after you log in through the dashboard)

**You must enter the new password privately through the legitimate dashboard interface.** After successful authentication, verify authenticated acceptance:

```bash
# 1. Confirm service is active
systemctl is-active rust-strategy-factory-dashboard.service

# 2. Confirm loopback-only listener
ss -tlnp | grep 3000
# Expected: 127.0.0.1:3000 only (not 0.0.0.0)

# 3. Unauthenticated endpoints should still be rejected
curl -s -o /dev/null -w "Unauthenticated /api/overview: HTTP %{http_code}\n" http://127.0.0.1:3000/api/overview
# Expected: 401

# 4. Authenticated endpoints — perform these steps yourself in a browser:
#    a. Open http://127.0.0.1:3000/ (sign-in page)
#    b. Enter your new password privately
#    c. After successful login, navigate to /api/overview or other protected endpoint
#    d. Confirm the endpoint returns HTTP 200 with your session cookie
#    e. Record the acceptance evidence locally (do not expose credentials)
```

## Security restrictions preserved

- Dashboard bound to `127.0.0.1:3000` only (not publicly accessible)
- Systemd unit enforces `NoNewPrivileges=yes`, `PrivateTmp=yes`, `ProtectSystem=strict`, `UMask=0077`
- Auth file mode 0600 (owner read/write only)
- Password never transmitted over network, never logged, never stored in plaintext
- No credentials requested or exposed in this document or any chat
