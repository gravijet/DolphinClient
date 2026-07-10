// Single source of truth for the version history, shared by the full
// /changelog page and the condensed changelog block on /download.

export interface ChangeEntry {
  v: string;
  date: string;
  items: string[];
}

export const CHANGES: ChangeEntry[] = [
  {
    v: "v0.13.0",
    date: "Aktuell · Server-Liste, Quick-Settings & mehr",
    items: [
      "Server-Liste im Launcher: Lieblingsserver speichern, einen Standard festlegen und mit einem Klick direkt beitreten",
      "Spiel-Schnelleinstellungen im Launcher: Render-Distanz, FPS-Limit, Sichtfeld, Helligkeit, VSync, GUI-Skalierung, Grafik-Preset und Discord Rich Presence — wirken beim nächsten Spielstart",
      "Profil mit Ganzkörper-Skin-Vorschau und einer Spielzeit-Historie (Sitzungs-Sparkline auf der Startseite)",
      "Web-Dashboard erweitert: neuer Server-Tab, Aktivitäts-Diagramm und die Spiel-Einstellungen — alles live aus dem laufenden Launcher",
      "Fix: der „Vollbild starten“-Schalter war ohne Funktion und schreibt die Einstellung jetzt korrekt in die options.json des Clients",
      "Eigene Changelog-Seite und überarbeitete Feature-Seite",
    ],
  },
  {
    v: "v0.12.0",
    date: "Neuer Launcher & Live-Dashboard",
    items: [
      "Komplett neu gestalteter Launcher: modernes, dunkles Design mit Seitenleiste, Profil, Cosmetics und Live-Status — im Stil moderner Clients",
      "Einstellungen speichern sich automatisch — es gibt keinen „Speichern“-Knopf mehr",
      "Neues Web-Dashboard, das sich live mit dem laufenden Launcher verbindet: aktives Konto, Version, Spielzeit und Einstellungen in Echtzeit",
      "Umzug auf die neue Domain example.invalid",
      "Cosmetics-Auswahl (Capes) im Launcher und Dashboard; Spielzeit- und Start-Statistik",
    ],
  },
  {
    v: "v0.3.0",
    date: "Nativer Client",
    items: [
      "Der Launcher startet jetzt den nativen DolphinClient (Rust + wgpu) statt Java-Minecraft — schneller Start, sehr hohe FPS, wenig RAM",
      "Kein Java/JDK mehr nötig: der Launcher lädt nur noch die Original-Texturen und -Modelle von Mojang, der Rest steckt im Client",
      "Deine Login-Session wird direkt an den Client übergeben — kein zweiter Login",
      "Server in den Einstellungen setzbar (direkt beitreten) oder Verbindungsbildschirm im Client",
    ],
  },
  {
    v: "v0.2.7",
    date: "Java-Launcher · LWJGL-Fix",
    items: [
      "Spielstart-Fix: nur die zur CPU-Architektur passenden LWJGL-Natives werden geladen (behebt lwjgl.dll-Fehler auf x64 endgültig)",
      "Keine Arch-Kollision mehr zwischen natives-windows / -arm64 / -x86",
    ],
  },
  {
    v: "v0.2.6",
    date: "Spielstart",
    items: [
      "Spielstart-Fix: LWJGL-Natives korrekt auf den Classpath (behebt lwjgl.dll-Fehler)",
      "Java-Version wird geprüft (26.1 braucht JDK 25) + Start-Log unter .minecraft/",
      "Abstürze werden jetzt direkt im Launcher angezeigt",
    ],
  },
  {
    v: "v0.2.5",
    date: "Login-Komfort",
    items: [
      "Login-Seite öffnet automatisch — direkter Link mit bereits eingetragenem Code",
    ],
  },
  {
    v: "v0.2.4",
    date: "Ohne Azure-App",
    items: [
      "Login funktioniert ohne eigene Azure-App (offizielle Launcher-ID via login.live.com)",
      "Behebt den login_with_xbox-403 — keine Freischaltung mehr nötig",
    ],
  },
  {
    v: "v0.2.3",
    date: "Browser-Login",
    items: ["Direkter Microsoft-Login im Browser (für eigene Azure-Apps)"],
  },
  {
    v: "v0.2.2",
    date: "Login-App",
    items: ["Neue Azure-App-Client-ID für den Microsoft-Login"],
  },
  {
    v: "v0.2.1",
    date: "Login-Konfiguration",
    items: [
      "Login-Tenant per DOLPHIN_MS_TENANT einstellbar (Standard: persönliche Konten)",
      "Klarere Fehlermeldungen bei der Microsoft-Anmeldung",
    ],
  },
  {
    v: "v0.2.0",
    date: "Nativer Launcher",
    items: [
      "Neuer nativer Launcher in Rust (egui) — startet in unter 0,5 s, ~11 MB",
      "Microsoft-Login, Spielstart & Update-Check im Launcher integriert",
      "Website-Redesign: Dashboard, mehr Content, Effekte & Animationen",
    ],
  },
  {
    v: "v0.1.0",
    date: "Erstes Release",
    items: [
      "26.1-Client mit FPS-, Koordinaten-, Uhr- und Zoom-Modulen",
      "Erster Launcher (Electron) mit Microsoft-Login und Spielstart",
    ],
  },
  {
    v: "v0.0.1",
    date: "Fundament",
    items: [
      "Monorepo mit Client, Launcher, Backend und Website",
      "Cosmetics-API (Capes) mit Account-Dashboard",
    ],
  },
];
