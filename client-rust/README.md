# DolphinClient (Rust)

A native, from-scratch Minecraft **26.1** client written in Rust — built for a
fast start, high FPS, and low memory use, even on weak hardware. It is
**multiplayer-first**: there is no world generation and no world saving. The
client connects to a real 26.1 server and renders the world the server sends.

- **Protocol / physics / auth:** [`azalea`](https://github.com/azalea-rs/azalea) `0.16.0+mc26.1`
- **Renderer:** `wgpu` 29 (Vulkan / Metal / DX12 / GL) + `winit` 0.30
- **HUD:** `egui` 0.35

For the full architecture, see [`../docs/rust-client/DESIGN.md`](../docs/rust-client/DESIGN.md)
and the azalea API notes in [`../docs/rust-client/api-notes/`](../docs/rust-client/api-notes/).

---

## Why native

The previous DolphinClient was a launcher/modpack around the stock Java client.
This is the game client itself, rebuilt so that:

- **Startup is fast** — assets (block models + textures) are baked once into a
  single atlas + per-state geometry table in ~0.4 s; no JVM warmup.
- **Frames are cheap** — sections are meshed in parallel on a `rayon` pool from
  lock-free snapshot copies, then uploaded once; the render thread only culls
  and draws. Rendering is camera-relative (f64 eye position, f32 per-draw
  offsets) so precision holds far from origin.
- **The network and the renderer never share a lock** — the azalea client runs
  on its own thread and communicates only through channels and plain-data
  snapshots.

## Requirements

- **Rust nightly** (pinned in `rust-toolchain.toml` — azalea's `simdnbt`
  needs nightly features). `rustup` will pick it up automatically.
- A **Vulkan / Metal / DX12 / GL** capable GPU. Software Vulkan
  (lavapipe/llvmpipe) works too, which is how the headless smoke test runs.
- Two asset inputs from a vanilla 26.1 install (auto-discovered from
  `.mc-cache/` upward from the working directory, or passed explicitly):
  - the **client jar** `client-26.1.jar` (block models + textures)
  - the **`blocks.json`** data-generator report (state id → block + properties);
    plain or gzipped.

## Build

```sh
cargo build --release
```

## Run

Join a server (offline mode):

```sh
cargo run --release -- --server play.example.net --username Dolphin
```

Microsoft account (azalea caches the token after the first browser login):

```sh
cargo run --release -- --server play.example.net --msa you@example.com
```

Omit `--server` to start at the connect screen. Explicit asset paths:

```sh
cargo run --release -- \
  --server localhost:25565 \
  --mc-jar /path/to/client-26.1.jar \
  --blocks-report /path/to/blocks.json.gz \
  --render-distance 12
```

### Controls

| Input | Action |
|---|---|
| `W` `A` `S` `D` | Move |
| `Space` | Jump |
| `Ctrl` (hold) | Sprint |
| `Shift` (hold) | Sneak |
| Mouse | Look |
| Left click | Mine / attack |
| Right click | Use / interact |
| `1`–`9` | Select hotbar slot |
| `T` or `Enter` | Open chat (prefix `/` for commands) |
| `F3` | Debug overlay |
| `Esc` | Pause menu (Back to Game / Options / Disconnect) |

## Headless smoke test

Renders PNG frames without a window (works over software Vulkan) and exits
non-zero on any failure — connect timeout, no chunks, or a uniform frame. This
is the end-to-end CI check.

```sh
cargo run --release -- \
  --offscreen --server localhost:25565 --username Dolphin \
  --out shots --frames 8
# optionally place blocks first, to exercise specific rendering paths:
#   --exec "/setblock ~ ~ ~2 glass"
```

## Test

```sh
cargo test          # 70 unit + integration tests
```

The bridge integration test (`live_connect_and_copy_sections`) connects to a
local server on `127.0.0.1:25565` if one is running, and **skips (passes)**
otherwise, so the suite is green without a server.

## Status

Working today: a Minecraft-style **title screen** (Singleplayer disabled —
multiplayer only — Multiplayer, Options, Quit), a Multiplayer connect screen, an
Options screen (FOV / sensitivity / render distance) and an Esc **pause menu**;
connect to a real 26.1 server (offline or Microsoft auth), receive and mesh
chunks with server light, render the world (opaque / cutout / translucent
layers, biome tint, atlas UVs), fly/walk with vanilla physics, chat,
mine/interact, hotbar, and an egui HUD (crosshair, hotbar, chat, F3). Rendering
and the menus are validated headlessly via the offscreen smoke test and
`--dump-menu`.

Not implemented (deliberately, for now): inventory/container UIs beyond the
hotbar, entity models (entities are tracked but not yet drawn), sound, particles.

---

*Private client. Not affiliated with Mojang or Microsoft.*
