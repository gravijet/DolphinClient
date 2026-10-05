# DolphinClient game client

Experimental Minecraft 26.1 client in Rust. Uses Azalea for the protocol and physics, wgpu for rendering, and egui for the interface.

Singleplayer runs a separate Pumpkin server. Multiplayer supports offline profiles and Microsoft accounts. Compatibility is still being tested.

## Requirements

- Rust nightly, pinned in `rust-toolchain.toml`.
- A GPU supporting Vulkan, Metal, DX12 or OpenGL.
- The vanilla `client-26.1.jar` and `blocks.json` data-generator report.

Asset files are discovered in `.mc-cache/` above the working directory, or supplied with `--mc-jar` and `--blocks-report`. Sound requires `--assets-dir` and `--asset-index`. Singleplayer requires a Pumpkin binary, supplied with `--server-binary` or discovered beside the client or in `.mc-cache/`.

## Build and run

Run these commands from `client-rust/`:

```sh
cargo build --release
cargo run --release
```

Connect to a local server with an offline profile:

```sh
cargo run --release -- --server localhost:25565 --username Dolphin
```

Use `--msa you@example.com` for Microsoft login. Run with `--help` for asset paths and other options.

## Controls

| Input | Action |
| --- | --- |
| WASD | Move |
| Space | Jump |
| Ctrl / Shift | Sprint / sneak |
| Mouse | Look |
| Left / right click | Mine or attack / use or interact |
| 1–9 | Select hotbar slot |
| T or Enter | Chat |
| F3 | Debug overlay |
| Esc | Pause menu |

## Tests

```sh
cargo test
cargo run --release -- \
    --offscreen --server localhost:25565 --username Dolphin \
    --out shots --frames 8
```

The offscreen test writes PNG frames and requires a running server. The `live_connect_and_copy_sections` integration test skips when no local server is available.

See [third-party notices](THIRD_PARTY_NOTICES.md) for Pumpkin licensing. Not affiliated with Mojang or Microsoft.
