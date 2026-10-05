# DolphinClient launcher

Rust launcher using eframe and egui. Manages Microsoft accounts, offline profiles, local cosmetics, client downloads and verified updates.

```sh
cargo run
cargo build --release
cargo test
```

Set `DOLPHIN_CLIENT_BIN` for a local client. Set `DOLPHIN_UPDATE_MANIFEST` and `DOLPHIN_CLIENT_URL` for your own distribution.

The default Microsoft login uses the device-code flow. An optional Azure application can be configured through `DOLPHIN_MS_CLIENT_ID`, `DOLPHIN_MS_MODE` and `DOLPHIN_MS_TENANT`. Offline profiles work only on servers configured to accept them.

Account tokens belong in the operating system's credential store or local account storage, outside Git.
