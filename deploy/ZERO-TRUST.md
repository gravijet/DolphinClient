# Securing the admin portal with Cloudflare Zero Trust

The admin portal lives at **`https://dolphinclient.de/admin`** and shows this
server's own measured numbers: downloads, update checks, visitors, the current
release, disk space, certificate lifetime, release history.

It is **not** protected by a password but by **identity** — independently, in
two places:

| Where | What |
|---|---|
| **Cloudflare Access** (edge) | Asks for identity before the request ever reaches this server (Google, GitHub, e-mail code …) and attaches a signed JWT. |
| **Origin gate** (this server) | `dolphinclient-access.service` verifies that JWT itself: signature against Cloudflare's keys, issuer, AUD, lifetime, e-mail. nginx asks it via `auth_request` on **every** request. |
| **Origin check** (this server) | Requests to `/admin` must arrive from Cloudflare's network (`conf.d/dolphinclient-cf-geo.conf`). |

Anyone who bypasses the Cloudflare address and talks to the server IP directly
has no token — and does not get in. If the gate is down or unconfigured, the
server answers **503**. Closed when in doubt, never open.

---

## 1. Create the Access application (Cloudflare dashboard, ~2 minutes)

1. Open **Zero Trust** (`one.dash.cloudflare.com`) → on first use pick a **team
   name**. The team domain is then `<team>.cloudflareaccess.com` — you need
   that value in a moment.
2. **Access → Applications → Add an application → Self-hosted**.
   * *Application name*: `DolphinClient Admin`
   * *Session duration*: e.g. 24 hours
   * *Public hostname*: domain `dolphinclient.de`, path **`admin`**
   * Add a second hostname with path **`admin-data`** (same application) so the
     data sits behind the same protection.
3. Add a **policy**: *Action* `Allow`, rule e.g. *Emails* →
   `gravijetbedwars@gmail.com`. (Anything that does not match is rejected by
   Cloudflare before it arrives here.)
4. Save, then click the application → **Overview → Application Audience (AUD)
   Tag** and copy it (a long hex string).

## 2. Enter it on the server

```bash
cd /home/benj/DolphinClient
sudo deploy/setup-zero-trust.sh \
  --team <your-team>.cloudflareaccess.com \
  --aud  <AUD tag> \
  --emails gravijetbedwars@gmail.com     # optional, an extra restriction
```

The script installs (or updates) everything needed:

* `/opt/dolphinclient/access-gate.mjs` + `dolphinclient-access.service`
  (the verifier on `127.0.0.1:8787`),
* `/opt/dolphinclient/admin-stats.mjs` + `dolphinclient-admin-stats.timer`
  (numbers refreshed every 10 minutes),
* the nginx snippets from `deploy/nginx/`,
* and reloads nginx (always `nginx -t` first).

It then reports `Gate: {"configured":true,…}` and
`https://dolphinclient.de/admin` asks you to sign in.

> The script **restarts** the gate rather than just starting it. A gate that is
> already running keeps serving with the old `access.env`, so a freshly entered
> team/AUD would silently have no effect and the portal would stay sealed.

## 3. Verify

```bash
curl -s http://127.0.0.1:8787/health                  # configured: true?
curl -s -o /dev/null -w '%{http_code}\n' \
  --resolve dolphinclient.de:443:127.0.0.1 -k \
  https://dolphinclient.de/admin/                     # straight to the origin: 403
curl -s -o /dev/null -w '%{redirect_url}\n' \
  https://dolphinclient.de/admin/                     # via Cloudflare: login redirect
systemctl status dolphinclient-access
node deploy/test-access-gate.mjs                      # 15 security tests
```

`deploy/test-access-gate.mjs` generates its own key pair and throws forged
tokens at the gate: foreign signature, expired, wrong AUD, wrong issuer,
`alg=none`, tampered payload, unconfigured. Every one of them **must** bounce.

---

## What if …

| Symptom | Cause / fix |
|---|---|
| `/admin` returns **503** "Sealed" | The gate is not running or not configured → `systemctl status dolphinclient-access`; if you just entered the values, make sure the gate was **restarted** (`systemctl restart dolphinclient-access`), then repeat step 2. |
| `/admin` returns **403** "Not authorised" | No token or an expired one: reload the page and sign in. Or the e-mail is not in `ACCESS_ALLOWED_EMAILS`. |
| **403** although signed in | The request did not come through Cloudflare (`$dolphin_from_cf`). Check the Cloudflare proxy (orange cloud); after a change to Cloudflare's IP list, regenerate `deploy/nginx/dolphinclient-cf-geo.conf`. |
| Portal shows "Could not read the statistics" | `systemctl start dolphinclient-admin-stats` and check the log. |
| The numbers are stale | The timer runs every 10 minutes: `systemctl list-timers dolphinclient-admin-stats`. |

## Files

| File | Purpose |
|---|---|
| `deploy/access-gate.mjs` | Verifies the Access JWT (`auth_request` backend, 127.0.0.1 only). |
| `deploy/admin-stats.mjs` | Builds `admin-data/stats.json` from the nginx logs, manifest and changelog. |
| `deploy/test-access-gate.mjs` | Security tests for the gate. |
| `deploy/setup-zero-trust.sh` | Installs the services + nginx configuration. |
| `deploy/nginx/dolphinclient-admin.conf` | The protected locations. |
| `deploy/nginx/dolphinclient-cf-geo.conf` | The Cloudflare origin check. |
| `website/app/admin/` | The portal page (static export). |

> **None of this lives in the web root** except the finished page and
> `admin-data/stats.json` — and both are reachable only behind the gate.
