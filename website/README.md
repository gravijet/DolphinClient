# DolphinClient — Website (`website/`)

Marketing-Seite, Download und Account-Bereich. **Next.js (App Router)**.

## Entwicklung

```bash
npm install                      # im Repo-Root (Workspaces)
npm run dev --workspace website  # http://localhost:3000
```

## Seiten

| Pfad | Inhalt |
|---|---|
| `/` | Landingpage (Hero, Features, ehrliche Performance-Aussage) |
| `/download` | Download pro Betriebssystem (aus dem Update-Feed, M4) |
| `/account` | Microsoft-Login + Cosmetics-Dashboard (M6) |

## Hinweise

- Performance-Aussagen ehrlich halten (FPS kommen aus Open-Source-Mods).
- Marken-Disclaimer ergänzen: „Nicht mit Mojang/Microsoft verbunden."
- Später: Cosmetic-Store + Zahlungen (Stripe, M7).
