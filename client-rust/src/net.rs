//! Server-address resolution — SRV redirect → A/AAAA — with a resolver tuned
//! for a game client, an in-process result cache and public-DNS fallback.
//!
//! Why this exists instead of just letting azalea resolve: azalea's built-in
//! `resolve_address` only returns a socket and then re-runs the *entire* lookup
//! again inside `start()`, and it uses whatever the system resolver defaults to
//! (which on a freshly-launched process, a cold OS cache, or a still-connecting
//! VPN is exactly what makes "the first connect hangs / fails"). We resolve
//! once, here, into a full [`ResolvedAddr`] — the original host is kept for the
//! login handshake (so virtual-host / proxy routing like BungeeCord forced-hosts
//! still works) and the socket is what we dial — and hand that straight to
//! azalea's `start()` / `ping_server()`, so the join path does *zero* extra DNS.
//!
//! What makes it fast and robust:
//! - IPv4-first (`Ipv4thenIpv6`): most Minecraft servers are v4-only, so we
//!   don't pay an AAAA round-trip or stall on a broken v6 route every connect.
//! - short 3s per-query timeout with 2 attempts, all configured nameservers
//!   queried in parallel — a single slow/dead resolver can't freeze the connect.
//! - proper SRV selection (lowest priority, then highest weight), vanilla-style
//!   (only on the default port).
//! - a 60s in-process cache of the final resolved address, so a
//!   disconnect→reconnect is instant.
//! - falls back to Cloudflare + Google + Quad9 when the OS resolver config
//!   can't be read.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use azalea::protocol::address::{ResolvedAddr, ServerAddr};
use hickory_resolver::config::{LookupIpStrategy, NameServerConfigGroup, ResolverConfig};
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::{Name, TokioResolver};
use parking_lot::Mutex;
use tracing::{debug, info, warn};

/// Vanilla only does an SRV lookup on the default port; matching that keeps
/// `host:1234` meaning "connect exactly there, no redirect".
const DEFAULT_PORT: u16 = 25565;

/// How long a fully-resolved address is reused before we look it up again.
/// Long enough that reconnecting after a kick/timeout is instant; short enough
/// that a server moving to a new IP is picked up within a minute.
const CACHE_TTL: Duration = Duration::from_secs(60);

/// Our own resolver — built once, shared. hickory keeps its own record cache
/// inside; this holds the connection pool and options.
static RESOLVER: LazyLock<TokioResolver> = LazyLock::new(build_resolver);

/// In-process cache of the final `host:port → ResolvedAddr`, keyed on the
/// address the user typed. Separate from hickory's record cache: it skips the
/// SRV+A chain entirely on a fast reconnect.
static CACHE: LazyLock<Mutex<HashMap<ServerAddr, (ResolvedAddr, Instant)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn build_resolver() -> TokioResolver {
    // Prefer the system resolver: it respects the user's LAN DNS, split-horizon
    // setups and any hosts-file entries. Fall back to fast public resolvers only
    // when the OS config can't be read (a locked-down box, a broken
    // /etc/resolv.conf) so a missing config never means "can't connect at all".
    let mut builder = match TokioResolver::builder_tokio() {
        Ok(b) => b,
        Err(e) => {
            warn!("System-DNS nicht lesbar ({e}); nutze Cloudflare + Google + Quad9");
            let mut group = NameServerConfigGroup::cloudflare();
            group.merge(NameServerConfigGroup::google());
            group.merge(NameServerConfigGroup::quad9());
            TokioResolver::builder_with_config(
                ResolverConfig::from_parts(None, vec![], group),
                TokioConnectionProvider::default(),
            )
        }
    };

    let opts = builder.options_mut();
    // A-record first: don't wait on AAAA (or dial an unreachable v6) for the
    // vast majority of servers that only listen on IPv4.
    opts.ip_strategy = LookupIpStrategy::Ipv4thenIpv6;
    // Snappier than the 5s default — a nameserver this slow is effectively
    // dead, so retry / move on instead of freezing the connect screen.
    opts.timeout = Duration::from_secs(3);
    opts.attempts = 2;
    // Query every configured nameserver in parallel and take the first answer.
    opts.num_concurrent_reqs = 3;
    opts.cache_size = 64;
    opts.edns0 = true;
    opts.try_tcp_on_error = true;
    // Don't let a server that publishes a 0s TTL force a fresh network lookup on
    // every single join within a session.
    opts.positive_min_ttl = Some(Duration::from_secs(30));

    builder.build()
}

/// Resolve `host` / `host:port` into a ready-to-dial [`ResolvedAddr`].
///
/// The returned value's `server` field is the *original* host:port (announced
/// to the server in the handshake); `socket` is the concrete IP:port to dial.
/// Pass the returned `&ResolvedAddr` straight to azalea — it won't re-resolve.
pub async fn resolve(address: &str) -> Result<ResolvedAddr, String> {
    let server = ServerAddr::try_from(address.trim())
        .map_err(|_| format!("Ungültige Serveradresse: „{address}“"))?;

    // A literal IP address needs no DNS at all.
    if let Ok(ip) = server.host.parse::<IpAddr>() {
        return Ok(ResolvedAddr { socket: SocketAddr::new(ip, server.port), server });
    }

    // Serve a still-fresh previous result instantly (fast reconnects).
    if let Some(hit) = cache_get(&server) {
        debug!(host = %server.host, socket = %hit.socket, "dns: cache hit");
        return Ok(hit);
    }

    // SRV redirect (vanilla-style, default port only). The original host is
    // still what we announce; only the socket target changes.
    let (lookup_host, port) = match srv_redirect(&server).await {
        Some((host, port)) => (host, port),
        None => (server.host.clone(), server.port),
    };

    let name = Name::from_ascii(&lookup_host)
        .map_err(|e| format!("Ungültiger Hostname „{lookup_host}“: {e}"))?;
    let ip = RESOLVER
        .lookup_ip(name)
        .await
        .map_err(|e| {
            format!("Server „{}“ wurde nicht gefunden (DNS: {e}). Adresse richtig geschrieben?", server.host)
        })?
        .iter()
        .next()
        .ok_or_else(|| format!("Keine IP-Adresse für „{}“ gefunden.", server.host))?;

    let resolved = ResolvedAddr { server: server.clone(), socket: SocketAddr::new(ip, port) };
    cache_put(&server, &resolved);
    info!(host = %server.host, socket = %resolved.socket, "dns: resolved");
    Ok(resolved)
}

/// Look up `_minecraft._tcp.<host>` and pick the best target per RFC 2782:
/// lowest priority wins, ties broken by highest weight. Returns `None` when
/// there's no SRV record (the common case — then we A-lookup the host itself).
async fn srv_redirect(server: &ServerAddr) -> Option<(String, u16)> {
    if server.port != DEFAULT_PORT {
        return None;
    }
    let query = format!("_minecraft._tcp.{}", server.host);
    let lookup = match RESOLVER.srv_lookup(query).await {
        Ok(l) => l,
        Err(e) => {
            debug!(host = %server.host, "dns: no SRV record ({e})");
            return None;
        }
    };
    // We deliberately don't do weighted-random load-spreading — for a single
    // game client "always pick the best-ranked target" is what you want.
    let best = lookup
        .iter()
        .min_by(|a, b| a.priority().cmp(&b.priority()).then(b.weight().cmp(&a.weight())))?;
    let target = best.target().to_ascii();
    let target = target.trim_end_matches('.').to_string();
    if target.is_empty() {
        // A single "." target is the RFC's "service explicitly not available".
        return None;
    }
    info!(host = %server.host, %target, port = best.port(), "dns: SRV redirect");
    Some((target, best.port()))
}

fn cache_get(server: &ServerAddr) -> Option<ResolvedAddr> {
    let now = Instant::now();
    let mut map = CACHE.lock();
    if let Some((addr, at)) = map.get(server) {
        if now.duration_since(*at) < CACHE_TTL {
            return Some(addr.clone());
        }
        map.remove(server);
    }
    None
}

fn cache_put(server: &ServerAddr, resolved: &ResolvedAddr) {
    CACHE.lock().insert(server.clone(), (resolved.clone(), Instant::now()));
}
