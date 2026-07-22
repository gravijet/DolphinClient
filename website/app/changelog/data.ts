// Single source of truth for the version history, shared by the full
// /changelog page and the condensed changelog block on /download.

export interface ChangeEntry {
  v: string;
  date: string;
  items: string[];
}

export const CHANGES: ChangeEntry[] = [
  {
    v: "v0.30.0",
    date: "Aktuell · Sunset glow & slime models",
    items: [
      "Sunrises and sunsets now glow: a soft warm haze radiates around the sun as it sits near the horizon at dawn and dusk, fading out by full day and hidden during rain — just like vanilla.",
      "Slimes and magma cubes now render as their green cube (scaled to the slime's size) instead of a plain coloured box.",
    ],
  },
  {
    v: "v0.29.0",
    date: "3D blocks in hand & weather",
    items: [
      "Held blocks now render as a real 3D cube in your hand — corner-toward-you, with the block's actual textures — instead of a flat icon. This is the usual view on block-heavy servers like bedwars.",
      "Weather: rain and thunderstorms are now shown, with falling rain streaks around you and the sky and daylight dimmed toward an overcast grey during a storm; the sun, moon and stars hide behind the clouds.",
    ],
  },
  {
    v: "v0.28.0",
    date: "First-person hand, sun, moon, stars and clouds",
    items: [
      "First-person hand: your arm and the item or block you are holding now show in the bottom-right corner, with a swing when you attack or mine, a raise when you switch items, and a gentle walk bob — exactly like vanilla.",
      "A living sky: a real sun and moon now cross the sky on the day cycle, stars fade in at night, and the sky colour shifts through sunrise, day, a warm sunset glow, and night.",
      "Clouds: a vanilla-style cloud layer drifts slowly across the sky and dims at night.",
      "Left-handed players now correctly hold the selected item in the shown hand.",
    ],
  },
  {
    v: "v0.27.0",
    date: "Launcher & Website verbessert",
    items: [
      "Launcher überarbeitet — stabiler und aufgeräumter",
      "Website aktualisiert und verfeinert",
    ],
  },
  {
    v: "v0.26.0",
    date: "Current · Launcher update & improvements",
    items: [
      "Launcher reworked — more stable and tidier",
    ],
  },
  {
    v: "v0.25.0",
    date: "Client & launcher improvements",
    items: [
      "Client improved: server connection, core & more",
      "Launcher reworked — more stable and tidier",
    ],
  },
  {
    v: "v0.24.0",
    date: "Brand-new launcher in the Lunar-Client style",
    items: [
      "Completely redesigned launcher in the style of modern clients (Lunar/Badlion)",
      "Left icon sidebar with navigation and an account card instead of a centered tab bar",
      "Cinematic home screen: your character as key art, a big headline and a prominent PLAY button",
      "Own embedded fonts (Outfit + Sora) instead of the default look",
      "Hand-drawn controls: sliders with gradient fill, toggles, segmented switches and a version dropdown",
      "Tidier colour palette: lighter raised panels and a restrained ocean accent",
    ],
  },
  {
    v: "v0.23.0",
    date: "Launcher: modern design, new Game tab & many features",
    items: [
      "New animated “Aurora” background with a fine measured grid and soft gradients",
      "Sliding accent underline between tabs, with hover effects",
      "New “Game” tab: render distance, frame-rate limit, VSync, field of view, brightness, GUI scale, graphics preset and fullscreen — written straight into options.json",
      "Home as a launch pad: your character on a lit stage with an idle animation and a glowing Play button",
      "Live stats on the home screen: total playtime, launches, average session and last played",
      "Play-history sparkline from the recorded session history",
      "Manage saved servers and join them from the home screen with a one-click chip",
      "Accounts show the live avatar of the active profile",
    ],
  },
  {
    v: "v0.22.0",
    date: "Client & launcher improvements",
    items: [
      "Client improved: HUD & display, menus & UI, rendering & graphics, audio & sound, and more",
      "Launcher reworked — more stable and tidier",
      "Improved build and release flow",
    ],
  },
  {
    v: "v0.19.0",
    date: "New design & real auto-updates",
    items: [
      "Website completely redesigned: a calm, precise “Sonar” look — deep neutral black, a fine measured grid and a single aqua accent. Clear and premium instead of effect overload",
      "Launcher rearranged: no more sidebar, simple text tabs on top (Home · Accounts · Settings), and “Play” sits right next to your character",
      "New settings: “Start with the system” (autostart) and truly automatic updates — found updates install without a click and the launcher briefly restarts",
      "Tidied up: no more cosmetics placeholders and no note about which launcher an account came from — only who's signed in matters",
      "Fresh typography and honest, up-to-date copy throughout the site and launcher",
    ],
  },
  {
    v: "v0.18.0",
    date: "Launcher cleaned up throughout",
    items: [
      "Redesigned launcher UI: calm, clear and tidy — dark surfaces, a single accent and a clear “Play” button. No effect ballast, only what you need",
      "Accounts front and centre: manage multiple Microsoft accounts or import accounts directly from other installed launchers — Vanilla, Lunar, Feather, Badlion, NoRisk, LabyMod, Prism, PolyMC and MultiMC",
      "More robust sign-in: if an account's sign-in fails once, the launcher pulls it automatically from another launcher on your PC — and if that doesn't work either, a clear “Sign in again” button appears",
      "Discord Rich Presence reworked and toggleable anytime in settings",
      "Tidied up: web dashboard and game settings removed from the launcher — settings now hold only what's useful",
    ],
  },
  {
    v: "v0.17.0",
    date: "Launcher fully in the “Prism” look",
    items: [
      "Launcher visually reinvented: the same lively “Aurora” as the site — drifting gradients on deep water-black, rising bubbles and light that follows the cursor",
      "Flowing, shimmering headlines and a live performance readout right on the home screen",
      "Animated comparison bars and a “Play” button with a flowing gradient and light reflection",
      "Discord Rich Presence: friends see in your Discord profile that you have DolphinClient open — in-game it still shows your server (without the raw IP)",
      "All launcher copy aligned with the site — clear, honest, jargon-free",
    ],
  },
  {
    v: "v0.16.0",
    date: "“Prism” — launcher & website reinvented",
    items: [
      "All-new design for site and launcher: a lively, shimmering “Aurora” of flowing gradients on deep water-black, plus glass surfaces and soft transitions",
      "New, honest copy without jargon — it's all about your benefit",
      "Home built around a performance comparison with three animated cards",
      "Tidy features page with a direct comparison, six clear benefits and a roadmap",
      "Launcher visually reinvented: the same lively look, an animated start area and clearer labels",
    ],
  },
  {
    v: "v0.15.0",
    date: "“Abyss” — dark design for site & launcher",
    items: [
      "All-new dark deep-ocean design for site and launcher: instrument-panel aesthetic with fine hairlines, mono labels for tech and versions and a single aqua accent",
      "Every text rewritten: sharper, more honest copy across the site and launcher",
      "New typography: Space Grotesk for headings, JetBrains Mono for spec labels, Inter for body text",
      "Home restructured: instrument readout panel, feature blueprint grid, spec-sheet comparison and timeline history",
      "Launcher overhauled: Abyss palette, datasheet cards, new hero, reworked fields and labels",
    ],
  },
  {
    v: "v0.14.0",
    date: "Light design for site & launcher",
    items: [
      "All-new light, modern website design in the style of current product pages — clear typography, plenty of whitespace, a subtle ocean gradient accent",
      "Launcher reworked: new tidy palette, ocean blue as the default accent, larger radii and a two-tone wordmark with a logo badge on the home screen",
      "Logo integrated consistently everywhere — visible, recognisable and size-optimised",
      "Fully responsive including a mobile menu; respects “prefers-reduced-motion”",
      "From now on, builds are published for Windows by default",
    ],
  },
  {
    v: "v0.13.0",
    date: "Server list, quick settings & more",
    items: [
      "Server list in the launcher: save favourite servers, set a default and join in one click",
      "Game quick settings in the launcher: render distance, FPS limit, field of view, brightness, VSync, GUI scale, graphics preset and Discord Rich Presence — applied on next launch",
      "Profile with a full-body skin preview and a playtime history (session sparkline on the home screen)",
      "Web dashboard expanded: new server tab, activity chart and the game settings — all live from the running launcher",
      "Fix: the “Start in fullscreen” toggle did nothing and now writes the setting correctly into the client's options.json",
      "New changelog page and reworked features page",
    ],
  },
  {
    v: "v0.12.0",
    date: "New launcher & live dashboard",
    items: [
      "Completely redesigned launcher: modern dark design with a sidebar, profile, cosmetics and live status — in the style of modern clients",
      "Settings save automatically — there's no more “Save” button",
      "New web dashboard that connects live to the running launcher: active account, version, playtime and settings in real time",
      "Moved to the new domain dolphinclient.de",
      "Cosmetics selection (capes) in the launcher and dashboard; playtime and launch stats",
    ],
  },
  {
    v: "v0.3.0",
    date: "Native client",
    items: [
      "The launcher now starts the native DolphinClient (Rust + wgpu) instead of Java Minecraft — fast startup, very high FPS, low RAM",
      "No more Java/JDK needed: the launcher only fetches the original textures and models from Mojang, the rest is in the client",
      "Your login session is passed straight to the client — no second login",
      "Server settable in settings (join directly) or via the connect screen in the client",
    ],
  },
  {
    v: "v0.2.7",
    date: "Java launcher · LWJGL fix",
    items: [
      "Launch fix: only the LWJGL natives matching your CPU architecture are loaded (fixes the lwjgl.dll error on x64 for good)",
      "No more arch collisions between natives-windows / -arm64 / -x86",
    ],
  },
  {
    v: "v0.2.6",
    date: "Game launch",
    items: [
      "Launch fix: LWJGL natives placed correctly on the classpath (fixes the lwjgl.dll error)",
      "Java version is checked (26.1 needs JDK 25) + a launch log under .minecraft/",
      "Crashes are now shown directly in the launcher",
    ],
  },
  {
    v: "v0.2.5",
    date: "Login convenience",
    items: [
      "The login page opens automatically — a direct link with the code already filled in",
    ],
  },
  {
    v: "v0.2.4",
    date: "Without an Azure app",
    items: [
      "Login works without your own Azure app (official launcher ID via login.live.com)",
      "Fixes the login_with_xbox 403 — no activation needed",
    ],
  },
  {
    v: "v0.2.3",
    date: "Browser login",
    items: ["Direct Microsoft login in the browser (for your own Azure apps)"],
  },
  {
    v: "v0.2.2",
    date: "Login app",
    items: ["New Azure app client ID for Microsoft login"],
  },
  {
    v: "v0.2.1",
    date: "Login configuration",
    items: [
      "Login tenant configurable via DOLPHIN_MS_TENANT (default: personal accounts)",
      "Clearer error messages during Microsoft sign-in",
    ],
  },
  {
    v: "v0.2.0",
    date: "Native launcher",
    items: [
      "New native launcher in Rust (egui) — starts in under 0.5 s, ~11 MB",
      "Microsoft login, game launch & update check built into the launcher",
      "Website redesign: dashboard, more content, effects & animations",
    ],
  },
  {
    v: "v0.1.0",
    date: "First release",
    items: [
      "26.1 client with FPS, coordinates, clock and zoom modules",
      "First launcher (Electron) with Microsoft login and game launch",
    ],
  },
  {
    v: "v0.0.1",
    date: "Foundation",
    items: [
      "Monorepo with client, launcher, backend and website",
      "Cosmetics API (capes) with an account dashboard",
    ],
  },
];
