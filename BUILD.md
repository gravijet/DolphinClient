# Building DolphinClient

The launcher uses stable Rust. The game client uses the nightly toolchain pinned in `client-rust/rust-toolchain.toml`.

```sh
cd launcher-native
cargo build --release
cargo test
```

```sh
cd client-rust
cargo build --release
cargo test
```

Build the website from the repository root:

```sh
npm install
npm run build:website
```

For local testing, point `DOLPHIN_CLIENT_BIN` at the client executable. Update and artifact URLs must be supplied for your own distribution.

Cross-compiling requires the target toolchain and platform libraries. Check the component READMEs for graphics and asset requirements.
