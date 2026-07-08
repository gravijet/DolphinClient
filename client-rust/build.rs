//! Embeds the DolphinClient icon into the Windows executable. On the Linux
//! cross-build this uses `x86_64-w64-mingw32-windres` from the mingw toolchain.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winres::WindowsResource::new();
        // The client icon (play-badge variant) — distinct from the launcher.
        res.set_icon("../assets/brand/dolphin-client.ico");
        if std::env::var("HOST").is_ok_and(|h| !h.contains("windows")) {
            res.set_windres_path("x86_64-w64-mingw32-windres");
            res.set_ar_path("x86_64-w64-mingw32-ar");
        }
        match res.compile() {
            Ok(()) => {
                // A resource-only archive exports no symbols, so the linker
                // would drop it — force the object file onto the link line.
                if let Ok(out) = std::env::var("OUT_DIR") {
                    println!("cargo:rustc-link-arg-bins={out}/resource.o");
                }
            }
            Err(e) => {
                println!("cargo:warning=Windows icon not embedded: {e}");
            }
        }
    }
    println!("cargo:rerun-if-changed=../assets/brand/dolphin-client.ico");
}
