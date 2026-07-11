// Single source of truth for the version history, shared by the full
// /changelog page and the condensed changelog block on /download.

export interface ChangeEntry {
  v: string;
  date: string;
  items: string[];
}

export const CHANGES: ChangeEntry[] = [
  {
    v: "v0.16.0",
    date: "Aktuell · „Prism“ — Launcher & Website neu erfunden",
    items: [
      "Komplett neues Design für Website und Launcher: eine lebendige, schimmernde „Aurora“ aus fließenden Farbverläufen auf tiefem Wasser-Schwarz, dazu Glas-Oberflächen, weiche Übergänge und animierte Vergleichsbalken — spürbar eigenständig statt Baukasten",
      "Neue, ehrliche Texte ganz ohne Fachbegriffe: Es geht nur noch um deinen Vorteil — mehr FPS, kürzere Ladezeit, weniger Arbeitsspeicher, direkt gegenübergestellt",
      "Startseite rund um den Leistungs-Vergleich gebaut: drei animierte Karten (FPS, Ladezeit, Speicher) zeigen DolphinClient neben normalem Minecraft",
      "Aufgeräumte Vorteils-Seite mit direktem Vergleich, sechs klaren Vorteilen und einer ehrlichen Roadmap",
      "Launcher grafisch neu erfunden: dieselbe lebendige Optik, ein animierter Startbereich und klarere, verständlichere Beschriftungen",
    ],
  },
  {
    v: "v0.15.0",
    date: "„Abyss“ — dunkles Design für Website & Launcher",
    items: [
      "Komplett neues, dunkles Deep-Ocean-Design für Website und Launcher: Instrument-Panel-Ästhetik mit feiner Hairline-Struktur, Mono-Beschriftungen für Technik und Versionen und einem einzigen Aqua-Akzent — bewusst kein Effekt-Overkill, klar handgemacht statt Baukasten",
      "Jeder Text neu geschrieben: schärfere, ehrlichere Copy über die ganze Seite und den Launcher — „eine Engine, kein Aufsatz“ statt Marketing-Floskeln",
      "Neue Typografie: Space Grotesk für Überschriften, JetBrains Mono für Spec-Labels, Inter für Fließtext",
      "Startseite neu strukturiert: Instrument-Readout-Panel, Feature-Blueprint-Raster, Spec-Sheet-Vergleich und Timeline-Verlauf",
      "Launcher grafisch runderneuert: Abyss-Palette, Datasheet-Karten, neuer Hero, überarbeitete Felder und Beschriftungen",
    ],
  },
  {
    v: "v0.14.0",
    date: "Helles Design für Website & Launcher",
    items: [
      "Komplett neues, helles und modernes Website-Design im Stil aktueller Produkt-Seiten — klare Typografie, viel Weißraum, eine dezente Ozean-Verlaufsakzentfarbe statt Effekt-Overkill",
      "Launcher überarbeitet: neue, aufgeräumte Palette, Ozean-Blau als Standard-Akzent, größere Radien und eine zweifarbige Wortmarke mit Logo-Badge auf der Startseite",
      "Logo überall stimmig eingebunden — sichtbar, gut erkennbar und datensparend optimiert (kleinere Dateigröße)",
      "Voll responsiv inklusive mobilem Menü; respektiert „prefers-reduced-motion“",
      "Ab sofort werden Builds standardmäßig für Windows veröffentlicht",
    ],
  },
  {
    v: "v0.13.0",
    date: "Server-Liste, Quick-Settings & mehr",
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
