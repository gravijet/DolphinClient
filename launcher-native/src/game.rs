//! Minecraft 26.1 launch pipeline: download vanilla version JSON + client jar,
//! libraries (with OS rules) + natives, assets, merge the Fabric profile,
//! install Fabric API + the DolphinClient mod, build the argument list and
//! spawn the JVM.
//!
//! Original game files come ONLY from Mojang / the Fabric meta service — never
//! self-hosted (Mojang EULA). The user must own the game.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use serde_json::Value;

use crate::auth::Session;
use crate::config::{self, Settings, TARGET_VERSION};
use crate::events::Event;

const VERSION_MANIFEST: &str = "https://launchermeta.mojang.com/mc/game/version_manifest_v2.json";
const RESOURCES: &str = "https://resources.download.minecraft.net";
const FABRIC_META: &str = "https://meta.fabricmc.net/v2";
const LOADER_VERSION: &str = "0.19.3";
// Fabric API is downloaded from Modrinth (not bundled — licence: redistribute
// with attribution, but we keep it simple and fetch it).
const FABRIC_API_FILE: &str = "fabric-api-0.153.0+26.1.2.jar";
const FABRIC_API_URL: &str =
    "https://cdn.modrinth.com/data/P7dR8mSH/versions/WC1KT7Yg/fabric-api-0.153.0%2B26.1.2.jar";

fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn http() -> Client {
    Client::builder()
        .user_agent(concat!(
            "DolphinClient-Launcher/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(Duration::from_secs(120))
        .build()
        .expect("failed to build HTTP client")
}

fn fetch_json(client: &Client, url: &str) -> Result<Value> {
    let res = client.get(url).send()?;
    if !res.status().is_success() {
        bail!("{} -> HTTP {}", url, res.status());
    }
    Ok(res.json()?)
}

/// Download `url` to `dest` (skips if it already exists). Writes to a temp file
/// first and renames, so an interrupted download never leaves a corrupt file.
fn download_file(client: &Client, url: &str, dest: &Path) -> Result<()> {
    if dest.exists() {
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let res = client.get(url).send()?;
    if !res.status().is_success() {
        bail!("Download fehlgeschlagen: {} (HTTP {})", url, res.status());
    }
    let bytes = res.bytes()?;
    let file_name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    let tmp = dest.with_file_name(format!("{}.part", file_name));
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// Evaluate the (optional) OS rules of a library / argument.
fn rules_allow(rules: Option<&Value>) -> bool {
    let rules = match rules {
        Some(Value::Array(a)) => a,
        _ => return true,
    };
    let mut allowed = false;
    for rule in rules {
        let mut matches = true;
        if let Some(name) = rule
            .get("os")
            .and_then(|o| o.get("name"))
            .and_then(|n| n.as_str())
        {
            matches = name == os_name();
        }
        if rule.get("features").is_some() {
            // Feature flags (demo / quick-play / resolution) — ignore here.
            matches = false;
        }
        if matches {
            allowed = rule.get("action").and_then(|a| a.as_str()) == Some("allow");
        }
    }
    allowed
}

/// `group:artifact:version` → `group/artifact/version/artifact-version.jar`
fn maven_path(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    let group = parts.first().copied().unwrap_or("");
    let artifact = parts.get(1).copied().unwrap_or("");
    let version = parts.get(2).copied().unwrap_or("");
    format!(
        "{}/{}/{}/{}-{}.jar",
        group.replace('.', "/"),
        artifact,
        version,
        artifact,
        version
    )
}

fn extract_natives(jar: &Path, natives_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(file)?;
    std::fs::create_dir_all(natives_dir)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.starts_with("META-INF") {
            continue;
        }
        // Flatten to the basename (maintainEntryPath = false).
        let base = Path::new(&name)
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(&name));
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(natives_dir.join(base), buf)?;
    }
    Ok(())
}

fn download_libraries(
    client: &Client,
    libraries: &Value,
    lib_dir: &Path,
    natives_dir: &Path,
) -> Result<Vec<PathBuf>> {
    let mut classpath = Vec::new();
    let arr = match libraries {
        Value::Array(a) => a,
        _ => return Ok(classpath),
    };
    for lib in arr {
        if !rules_allow(lib.get("rules")) {
            continue;
        }

        // Vanilla style: downloads.artifact with url + path.
        if let Some(artifact) = lib.get("downloads").and_then(|d| d.get("artifact")) {
            let url = artifact.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let path = artifact.get("path").and_then(|p| p.as_str());
            if !url.is_empty() {
                if let Some(path) = path {
                    let dest = lib_dir.join(path);
                    download_file(client, url, &dest)?;
                    if path.contains("natives-") {
                        extract_natives(&dest, natives_dir)?;
                    } else {
                        classpath.push(dest);
                    }
                    continue;
                }
            }
        }

        // Fabric style: name + maven base url.
        if let (Some(name), Some(url)) = (
            lib.get("name").and_then(|n| n.as_str()),
            lib.get("url").and_then(|u| u.as_str()),
        ) {
            let rel = maven_path(name);
            let dest = lib_dir.join(&rel);
            let full = format!("{}/{}", url.trim_end_matches('/'), rel);
            download_file(client, &full, &dest)?;
            classpath.push(dest);
        }
    }
    Ok(classpath)
}

fn download_assets(
    client: &Client,
    asset_index: &Value,
    root: &Path,
    tx: &Sender<Event>,
) -> Result<String> {
    let id = asset_index
        .get("id")
        .and_then(|i| i.as_str())
        .unwrap_or("legacy")
        .to_string();
    let url = asset_index
        .get("url")
        .and_then(|u| u.as_str())
        .context("Asset-Index ohne URL")?;

    let index_file = root
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", id));
    download_file(client, url, &index_file)?;

    let index: Value = serde_json::from_str(&std::fs::read_to_string(&index_file)?)?;
    let objects_dir = root.join("assets").join("objects");

    if let Some(objects) = index.get("objects").and_then(|o| o.as_object()) {
        let total = objects.len().max(1);
        for (i, (_key, obj)) in objects.iter().enumerate() {
            if let Some(hash) = obj.get("hash").and_then(|h| h.as_str()) {
                if hash.len() >= 2 {
                    let sub = &hash[0..2];
                    let dest = objects_dir.join(sub).join(hash);
                    download_file(client, &format!("{}/{}/{}", RESOURCES, sub, hash), &dest)?;
                }
            }
            if i % 40 == 0 {
                let _ = tx.send(Event::Progress(0.35 + 0.5 * (i as f32 / total as f32)));
            }
        }
    }
    Ok(id)
}

fn substitute(arg: &str, vars: &HashMap<String, String>) -> String {
    let mut s = arg.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("${{{}}}", k), v);
    }
    s
}

fn collect_args(section: Option<&Value>, vars: &HashMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    let arr = match section {
        Some(Value::Array(a)) => a,
        _ => return out,
    };
    for arg in arr {
        match arg {
            Value::String(s) => out.push(substitute(s, vars)),
            Value::Object(_) => {
                if !rules_allow(arg.get("rules")) {
                    continue;
                }
                match arg.get("value") {
                    Some(Value::String(s)) => out.push(substitute(s, vars)),
                    Some(Value::Array(vs)) => {
                        for v in vs {
                            if let Some(s) = v.as_str() {
                                out.push(substitute(s, vars));
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    out
}

/// Candidate directories that may contain the bundled DolphinClient mod jar.
fn bundled_mods_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            dirs.push(dir.join("mods"));
            dirs.push(dir.join("resources").join("mods"));
        }
    }
    dirs.push(PathBuf::from("resources/mods"));
    dirs
}

fn install_mods(client: &Client, root: &Path, tx: &Sender<Event>) -> Result<()> {
    let mods_dir = root.join("mods");
    std::fs::create_dir_all(&mods_dir)?;

    let _ = tx.send(Event::Status("Fabric API installieren …".into()));
    download_file(client, FABRIC_API_URL, &mods_dir.join(FABRIC_API_FILE))?;

    for src in bundled_mods_dirs() {
        if !src.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&src)?.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jar") {
                if let Some(name) = p.file_name() {
                    let _ = std::fs::copy(&p, mods_dir.join(name));
                    let _ = tx.send(Event::Log(format!(
                        "Mod installiert: {}",
                        name.to_string_lossy()
                    )));
                }
            }
        }
    }
    Ok(())
}

fn get_vanilla_version(client: &Client) -> Result<Value> {
    let manifest = fetch_json(client, VERSION_MANIFEST)?;
    let versions = manifest
        .get("versions")
        .and_then(|v| v.as_array())
        .context("Manifest ohne Versionsliste")?;
    let entry = versions
        .iter()
        .find(|v| v.get("id").and_then(|i| i.as_str()) == Some(TARGET_VERSION))
        .with_context(|| format!("Version {} nicht im Manifest gefunden.", TARGET_VERSION))?;
    let url = entry
        .get("url")
        .and_then(|u| u.as_str())
        .context("Versionseintrag ohne URL")?;
    fetch_json(client, url)
}

/// Run the whole pipeline and spawn the game. Returns once the JVM is started.
pub fn launch(session: &Session, settings: &Settings, tx: &Sender<Event>) -> Result<()> {
    let client = http();
    let root = config::minecraft_dir();
    let lib_dir = root.join("libraries");
    let natives_dir = root.join("versions").join(TARGET_VERSION).join("natives");
    std::fs::create_dir_all(&natives_dir)?;

    // 1. Vanilla version JSON + client jar.
    let _ = tx.send(Event::Status("Versions-Manifest laden …".into()));
    let _ = tx.send(Event::Progress(0.05));
    let version = get_vanilla_version(&client)?;

    let _ = tx.send(Event::Status("Client-JAR laden …".into()));
    let client_jar = root
        .join("versions")
        .join(TARGET_VERSION)
        .join(format!("{}.jar", TARGET_VERSION));
    let client_url = version["downloads"]["client"]["url"]
        .as_str()
        .context("Client-JAR-URL fehlt")?;
    download_file(&client, client_url, &client_jar)?;
    let _ = tx.send(Event::Progress(0.15));

    // 2. Vanilla libraries (+ natives) → classpath.
    let _ = tx.send(Event::Status("Bibliotheken laden …".into()));
    let mut classpath = download_libraries(&client, &version["libraries"], &lib_dir, &natives_dir)?;
    classpath.push(client_jar.clone());
    let _ = tx.send(Event::Progress(0.35));

    // 3. Assets.
    let _ = tx.send(Event::Status("Assets laden …".into()));
    let asset_index_id = download_assets(&client, &version["assetIndex"], &root, tx)?;
    let _ = tx.send(Event::Progress(0.9));

    // 4. Fabric profile: loader libraries + mainClass.
    let _ = tx.send(Event::Status("Fabric einrichten …".into()));
    let fabric = fetch_json(
        &client,
        &format!(
            "{}/versions/loader/{}/{}/profile/json",
            FABRIC_META, TARGET_VERSION, LOADER_VERSION
        ),
    )?;
    for cp in download_libraries(&client, &fabric["libraries"], &lib_dir, &natives_dir)? {
        if !classpath.contains(&cp) {
            classpath.push(cp);
        }
    }
    let main_class = fabric
        .get("mainClass")
        .and_then(|m| m.as_str())
        .or_else(|| version.get("mainClass").and_then(|m| m.as_str()))
        .context("keine mainClass gefunden")?
        .to_string();

    // 5. Fabric API + bundled DolphinClient mod.
    install_mods(&client, &root, tx)?;

    // 6. Build arguments.
    let sep = if cfg!(target_os = "windows") {
        ";"
    } else {
        ":"
    };
    let classpath_str = classpath
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(sep);

    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("auth_player_name".into(), session.username.clone());
    vars.insert("version_name".into(), TARGET_VERSION.into());
    vars.insert("game_directory".into(), root.to_string_lossy().into_owned());
    vars.insert(
        "assets_root".into(),
        root.join("assets").to_string_lossy().into_owned(),
    );
    vars.insert("assets_index_name".into(), asset_index_id);
    vars.insert("auth_uuid".into(), session.uuid.clone());
    vars.insert("auth_access_token".into(), session.access_token.clone());
    vars.insert("clientid".into(), String::new());
    vars.insert("auth_xuid".into(), String::new());
    vars.insert("user_type".into(), "msa".into());
    vars.insert("version_type".into(), "release".into());
    vars.insert(
        "natives_directory".into(),
        natives_dir.to_string_lossy().into_owned(),
    );
    vars.insert("launcher_name".into(), "DolphinClient".into());
    vars.insert("launcher_version".into(), "0.2.5".into());
    vars.insert("classpath".into(), classpath_str.clone());

    let mut jvm_args = collect_args(version.get("arguments").and_then(|a| a.get("jvm")), &vars);
    jvm_args.extend(collect_args(
        fabric.get("arguments").and_then(|a| a.get("jvm")),
        &vars,
    ));
    jvm_args.push(format!("-Xmx{}G", settings.ram_gb.max(1)));

    // Fallback for the legacy argument format (26.1 uses `arguments`).
    if version.get("arguments").is_none() {
        jvm_args.push(format!(
            "-Djava.library.path={}",
            natives_dir.to_string_lossy()
        ));
        jvm_args.push("-cp".into());
        jvm_args.push(classpath_str.clone());
    }

    let mut game_args = collect_args(version.get("arguments").and_then(|a| a.get("game")), &vars);
    if settings.fullscreen {
        game_args.push("--fullscreen".into());
    }

    // 7. Start the JVM.
    let java = settings.java_bin();
    let _ = tx.send(Event::Status(format!(
        "Starte Minecraft {} …",
        TARGET_VERSION
    )));
    let _ = tx.send(Event::Progress(1.0));
    let _ = tx.send(Event::Log(format!(
        "java ({} JVM- / {} Spiel-Argumente) → {}",
        jvm_args.len(),
        game_args.len(),
        main_class
    )));

    let child = Command::new(&java)
        .args(&jvm_args)
        .arg(&main_class)
        .args(&game_args)
        .current_dir(&root)
        .spawn()
        .with_context(|| {
            format!(
                "Java-Start fehlgeschlagen ({}). Ist JDK 25 installiert bzw. der Pfad gesetzt?",
                java
            )
        })?;

    // Detach — the game runs independently of the launcher.
    drop(child);
    let _ = tx.send(Event::Launched);
    Ok(())
}
