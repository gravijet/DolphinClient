// Single source of truth for the version history, shared by the full
// /changelog page and the condensed changelog block on /download.

export interface ChangeEntry {
  v: string;
  date: string;
  items: string[];
}

export const CHANGES: ChangeEntry[] = [
  {
    v: "v0.26.0",
    date: "Aktuell · Launcher-Update & Verbesserungen",
    items: [
      "Launcher überarbeitet — stabiler und aufgeräumter",
    ],
  },
  {
    v: "v0.25.0",
    date: "Client & Launcher verbessert",
    items: [
      "Client verbessert: Server-Verbindung, Kern & Sonstiges",
      "Launcher überarbeitet — stabiler und aufgeräumter",
    ],
  },
  {
    v: "v0.24.0",
    date: "Launcher komplett neu im Lunar-Client-Stil",
    items: [
      "Komplett neu gestalteter Launcher im Stil moderner Clients (Lunar/Badlion)",
      "Linke Icon-Seitenleiste mit Navigation und Konto-Karte statt zentrierter Tab-Leiste",
      "Kinematische Startseite: dein Charakter als Key-Art, große Headline und prominenter LAUNCH-Button",
      "Eigene, eingebettete Schriftarten (Outfit + Sora) statt der Standard-Optik",
      "Selbstgezeichnete Bedienelemente: Slider mit Verlaufsfüllung, Toggles, Segment-Umschalter und Versions-Dropdown",
      "Aufgeräumte Farbwelt: hellere abgesetzte Panels und ein zurückhaltender Ozean-Akzent",
    ],
  },
  {
    v: "v0.23.0",
    date: "Launcher: modernes Design, neuer Spiel-Tab & viele Features",
    items: [
      "Neuer animierter „Aurora\"-Hintergrund mit feinem Messraster und sanften Verläufen",
      "Gleitende Akzent-Unterstreichung zwischen den Tabs samt Hover-Effekten",
      "Neuer Tab „Spiel\": Sichtweite, Bildrate-Limit, VSync, Sichtfeld, Helligkeit, GUI-Größe, Grafik-Preset und Vollbild – direkt in die options.json geschrieben",
      "Startseite als Startrampe: Charakter auf beleuchteter Bühne mit Idle-Animation und glühendem Play-Button",
      "Live-Statistiken auf der Startseite: Gesamt-Spielzeit, Starts, Ø Sitzung und zuletzt gespielt",
      "Spielverlauf-Sparkline aus der erfassten Sitzungshistorie",
      "Gespeicherte Server verwalten und per Ein-Klick-Chip auf der Startseite beitreten",
      "Konten zeigen den Live-Avatar des aktiven Profils",
    ],
  },
  {
    v: "v0.22.0",
    date: "Client & Launcher verbessert",
    items: [
      "Client verbessert: HUD & Anzeige, Menüs & UI, Rendering & Grafik, Audio & Sound u. a.",
      "Launcher überarbeitet — stabiler und aufgeräumter",
      "Build- und Veröffentlichungs-Ablauf verbessert",
    ],
  },
  {
    v: "v0.19.0",
    date: "Neues Design & echte Auto-Updates",
    items: [
      "Website komplett neu gestaltet: ein ruhiges, präzises „Sonar“-Design — tiefes Neutral-Schwarz, ein feines Messraster, ein einziger Aqua-Akzent und eine sich selbst zeichnende FPS-Linie. Klar und wertig statt Effekt-Overkill",
      "Launcher neu angeordnet: keine Seitenleiste mehr, oben schlichte Text-Reiter (Start · Konten · Einstellungen), und „Spielen“ sitzt direkt bei deinem Charakter",
      "Neue Einstellungen: „Mit dem System starten“ (Autostart) und wirklich automatische Updates — gefundene Updates werden ohne Klick eingespielt und der Launcher startet kurz neu",
      "Aufgeräumt: keine Cosmetics-Platzhalter mehr und kein Hinweis, aus welchem Launcher ein Konto stammt — es zählt nur noch, wer angemeldet ist",
      "Frische Typografie und durchgehend ehrliche, aktuelle Texte auf Website und im Launcher",
    ],
  },
  {
    v: "v0.18.0",
    date: "Launcher rundum aufgeräumt",
    items: [
      "Launcher-Oberfläche neu gestaltet: ruhig, klar und aufgeräumt — dunkle Flächen, ein einziger Akzent und ein deutlicher „Spielen“-Knopf. Kein Effekt-Ballast, nur was du wirklich brauchst",
      "Konten im Mittelpunkt: mehrere Microsoft-Konten verwalten oder Konten direkt aus anderen installierten Launchern übernehmen — Vanilla, Lunar, Feather, Badlion, NoRisk, LabyMod, Prism, PolyMC und MultiMC",
      "Anmeldung robuster: klappt die Anmeldung eines Kontos einmal nicht, holt der Launcher es automatisch aus einem anderen Launcher auf deinem PC — und wenn auch das nicht geht, erscheint ein klarer „Neu anmelden“-Knopf",
      "Discord Rich Presence überarbeitet und jederzeit in den Einstellungen ein- oder ausschaltbar",
      "Aufgeräumt: Web-Dashboard und Spiel-Einstellungen aus dem Launcher entfernt — die Einstellungen enthalten jetzt nur noch Nützliches",
    ],
  },
  {
    v: "v0.17.0",
    date: "Launcher komplett im „Prism“-Look",
    items: [
      "Launcher grafisch neu erfunden: dieselbe lebendige „Aurora“ wie die Website — driftende Farbverläufe auf tiefem Wasser-Schwarz, aufsteigende Bläschen und Licht, das dem Mauszeiger folgt",
      "Fließende, schimmernde Überschriften und eine Live-Leistungsanzeige direkt auf der Startseite: FPS, Ladezeit und Speicher auf einen Blick",
      "Animierte Vergleichsbalken (FPS, Ladezeit, Speicher) und ein „Spielen“-Knopf mit fließendem Farbverlauf und Licht-Reflex",
      "Discord Rich Presence: Freunde sehen in deinem Discord-Profil, dass du DolphinClient offen hast — im Spiel zeigt es weiterhin deinen Server (ohne rohe IP)",
      "Alle Texte im Launcher an die Website angeglichen — klar, ehrlich, ohne Fachbegriffe",
    ],
  },
  {
    v: "v0.16.0",
    date: "„Prism“ — Launcher & Website neu erfunden",
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
      "Umzug auf die neue Domain dolphinclient.de",
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
