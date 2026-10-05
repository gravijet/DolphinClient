# DolphinClient

Experimental Minecraft client and launcher written in Rust, with a Next.js website.

The client is still incomplete. Singleplayer uses a separate Pumpkin server process and requires its binary. Server compatibility and rendering need further testing across hardware.

## Development

```sh
cd launcher-native
cargo run
```

The game client uses the nightly toolchain pinned in `client-rust/rust-toolchain.toml`:

```sh
cd client-rust
cargo build --release
```

Website commands run from the repository root:

```sh
npm install
npm run dev:website
```

Set `DOLPHIN_CLIENT_BIN` to a local client build for development. Configure update and download URLs through `DOLPHIN_UPDATE_MANIFEST` and `DOLPHIN_CLIENT_URL`.

See [BUILD.md](BUILD.md) for build requirements and the component READMEs for details. Not affiliated with Mojang or Microsoft.
