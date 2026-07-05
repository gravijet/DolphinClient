# Keine GitHub-Builds mehr

Die früheren Workflows `ci.yml` (Typecheck + Buildchecks) und `release.yml`
(Bau von Launcher **und** Client für Windows/macOS/Linux + Upload ins
GitHub-Release) wurden **bewusst entfernt**.

**Es wird nichts mehr auf GitHub gebaut.** Launcher und Client werden ab jetzt
**lokal** gebaut und von Hand veröffentlicht.

Die komplette, Schritt-für-Schritt-Anleitung dafür steht in:

    ANLEITUNG-BUILD.md   (im Wurzelverzeichnis des Repos)

Falls du später doch wieder automatisch bauen möchtest, lege einfach wieder eine
`*.yml`-Datei in diesem Ordner an — dieser `README.md` löst nichts aus (GitHub
Actions führt nur `*.yml`/`*.yaml` aus).
