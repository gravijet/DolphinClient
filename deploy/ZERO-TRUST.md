# Admin-Portal mit Cloudflare Zero Trust absichern

Das Admin-Portal liegt unter **`https://dolphinclient.de/admin`** und zeigt die
gemessenen Zahlen dieses Servers: Downloads, Update-Abfragen, Besucher, das
aktuelle Release, Plattenplatz, Zertifikatslaufzeit, Release-Historie.

Es ist **nicht** durch ein Passwort geschützt, sondern durch **Identität** —
und zwar an zwei Stellen unabhängig voneinander:

| Wo | Was |
|---|---|
| **Cloudflare Access** (Rand) | Fragt vor dem Server nach der Identität (Google, GitHub, E-Mail-Code …) und hängt ein signiertes JWT an die Anfrage. |
| **Origin-Gate** (dieser Server) | `dolphinclient-access.service` prüft dieses JWT selbst: Signatur gegen die Cloudflare-Schlüssel, Aussteller, AUD, Laufzeit, E-Mail. nginx fragt es per `auth_request` bei **jeder** Anfrage. |
| **Herkunft** (dieser Server) | Anfragen an `/admin` müssen aus dem Cloudflare-Netz kommen (`conf.d/dolphinclient-cf-geo.conf`). |

Wer die Cloudflare-Adresse umgeht und direkt die Server-IP anspricht, hat kein
Token — und kommt nicht rein. Läuft das Gate nicht oder fehlt die
Konfiguration, antwortet der Server **503**. Im Zweifel zu, nie offen.

---

## 1. Access-Anwendung anlegen (im Cloudflare-Dashboard, ~2 Minuten)

1. **Zero Trust** öffnen (`one.dash.cloudflare.com`) → beim ersten Mal einen
   **Team-Namen** wählen. Die Team-Domain heißt dann
   `<team>.cloudflareaccess.com` — den Wert brauchst du gleich.
2. **Access → Applications → Add an application → Self-hosted**.
   * *Application name*: `DolphinClient Admin`
   * *Session duration*: z. B. 24 Stunden
   * *Public hostname*: Domain `dolphinclient.de`, Pfad **`admin`**
   * Eine zweite Domain mit Pfad **`admin-data`** hinzufügen (dieselbe
     Anwendung), damit die Daten unter demselben Schutz stehen.
3. **Policy** anlegen: *Action* `Allow`, Regel z. B.
   *Emails* → `gravijetbedwars@gmail.com`. (Alles, was nicht passt, wird von
   Cloudflare abgewiesen, bevor es hier ankommt.)
4. Anwendung speichern, dann in der Übersicht auf die Anwendung klicken →
   **Overview → Application Audience (AUD) Tag** kopieren (langer Hex-String).

## 2. Auf dem Server eintragen

```bash
cd /home/benj/DolphinClient
sudo deploy/setup-zero-trust.sh \
  --team <dein-team>.cloudflareaccess.com \
  --aud  <AUD-Tag> \
  --emails gravijetbedwars@gmail.com     # optional, zusätzliche Sperre
```

Das Skript installiert (bzw. aktualisiert) alles Nötige:

* `/opt/dolphinclient/access-gate.mjs` + `dolphinclient-access.service`
  (Prüfdienst auf `127.0.0.1:8787`),
* `/opt/dolphinclient/admin-stats.mjs` + `dolphinclient-admin-stats.timer`
  (Zahlen alle 10 Minuten neu),
* die nginx-Schnipsel aus `deploy/nginx/`,
* und lädt nginx neu (vorher immer `nginx -t`).

Danach meldet es `Gate: {"configured":true,…}` — und
`https://dolphinclient.de/admin` fragt beim Aufruf nach der Anmeldung.

## 3. Prüfen

```bash
curl -s http://127.0.0.1:8787/health          # configured: true?
curl -si https://dolphinclient.de/admin/ | head -1   # ohne Anmeldung: 403
systemctl status dolphinclient-access
node deploy/test-access-gate.mjs              # 15 Sicherheitstests
```

`deploy/test-access-gate.mjs` erzeugt ein eigenes Schlüsselpaar und wirft dem
Gate gefälschte Token vor die Füße: fremd signiert, abgelaufen, falsche AUD,
falscher Aussteller, `alg=none`, manipulierte Nutzdaten, unkonfiguriert. Jedes
davon **muss** abgewiesen werden.

---

## Was wenn …

| Symptom | Ursache / Lösung |
|---|---|
| `/admin` liefert **503** „Sealed" | Gate läuft nicht oder ist nicht konfiguriert → `systemctl status dolphinclient-access`, ggf. Schritt 2 wiederholen. |
| `/admin` liefert **403** „Not authorised" | Kein/abgelaufenes Token: Seite neu laden und anmelden. Oder die E-Mail steht nicht in `ACCESS_ALLOWED_EMAILS`. |
| **403** obwohl angemeldet | Anfrage kam nicht über Cloudflare (`$dolphin_from_cf`). Cloudflare-Proxy (orange Wolke) prüfen; nach einer Änderung der Cloudflare-IP-Liste `deploy/nginx/dolphinclient-cf-geo.conf` neu erzeugen. |
| Portal zeigt „Could not read the statistics" | `systemctl start dolphinclient-admin-stats` und Log prüfen. |
| Zahlen sind alt | Der Timer läuft alle 10 Minuten: `systemctl list-timers dolphinclient-admin-stats`. |

## Dateien

| Datei | Zweck |
|---|---|
| `deploy/access-gate.mjs` | Prüft das Access-JWT (`auth_request`-Backend, nur 127.0.0.1). |
| `deploy/admin-stats.mjs` | Erzeugt `admin-data/stats.json` aus nginx-Logs, Manifest, Changelog. |
| `deploy/test-access-gate.mjs` | Sicherheitstests für das Gate. |
| `deploy/setup-zero-trust.sh` | Installiert Dienste + nginx-Konfiguration. |
| `deploy/nginx/dolphinclient-admin.conf` | Die geschützten Locations. |
| `deploy/nginx/dolphinclient-cf-geo.conf` | Cloudflare-Herkunftsprüfung. |
| `website/app/admin/` | Die Portalseite (statischer Export). |

> **Nichts davon liegt im Web-Root**, außer der fertigen Seite und
> `admin-data/stats.json` — und beides ist nur hinter dem Gate erreichbar.
