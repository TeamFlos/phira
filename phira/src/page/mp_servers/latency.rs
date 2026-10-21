use anyhow::{bail, Result};
use phira_mp_client::{resolver, Client};
use prpr::config::MpServer;
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{lookup_host, TcpStream},
    sync::Mutex,
    task::JoinSet,
    time::{timeout, Instant},
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(10);
const ADDRESS_CACHE_LIFETIME: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
pub enum PingState {
    Checking,
    Ready(Duration),
    TimedOut,
    Unreachable,
}

#[derive(Default)]
struct AddressCache {
    resolved: Option<(Instant, Vec<SocketAddr>)>,
}

impl AddressCache {
    async fn resolve(&mut self, address: &str) -> Result<Vec<SocketAddr>> {
        if let Some((resolved_at, addresses)) = &self.resolved {
            if resolved_at.elapsed() < ADDRESS_CACHE_LIFETIME {
                return Ok(addresses.clone());
            }
        }
        let authority = address.parse::<http::uri::Authority>()?;
        // Preserve the multiplayer client's SRV and IPv6 handling.
        let host = resolver::resolve(&authority).await?;
        let addresses: Vec<_> = lookup_host(host).await?.collect();
        if addresses.is_empty() {
            bail!("server address did not resolve");
        }
        self.resolved = Some((Instant::now(), addresses.clone()));
        Ok(addresses)
    }
}

async fn probe(address: &str, cache: &Mutex<AddressCache>) -> PingState {
    let mut cache = cache.lock().await;
    let state = probe_attempt(address, &mut cache).await;
    let state = if matches!(state, PingState::TimedOut) {
        // A retry reuses resolved IPs. It never creates a third attempt.
        probe_attempt(address, &mut cache).await
    } else {
        state
    };
    if matches!(state, PingState::TimedOut | PingState::Unreachable) {
        cache.resolved = None;
    }
    state
}

async fn probe_attempt(address: &str, cache: &mut AddressCache) -> PingState {
    let addresses = match timeout(RESOLVE_TIMEOUT, cache.resolve(address)).await {
        Ok(Ok(addresses)) => addresses,
        Ok(Err(_err)) => {
            return PingState::Unreachable;
        }
        Err(_) => {
            return PingState::TimedOut;
        }
    };
    probe_once(&addresses).await
}

async fn probe_once(addresses: &[SocketAddr]) -> PingState {
    // DNS, connection setup, and Pong each have their own deadline.
    let socket = match timeout(PROBE_TIMEOUT, TcpStream::connect(addresses)).await {
        Ok(Ok(socket)) => socket,
        Ok(Err(_err)) => {
            return PingState::Unreachable;
        }
        Err(_) => {
            return PingState::TimedOut;
        }
    };
    // Finish before the client's automatic heartbeat starts. No login is performed.
    match timeout(PROBE_TIMEOUT, async {
        let client = Client::new(socket).await?;
        client.ping().await
    })
    .await
    {
        Ok(Ok(delay)) => PingState::Ready(delay),
        Ok(Err(err)) if err.is::<tokio::time::error::Elapsed>() => PingState::TimedOut,
        Ok(Err(_err)) => {
            PingState::Unreachable
        }
        Err(_) => {
            PingState::TimedOut
        }
    }
}

struct ServerPing {
    state: PingState,
    task: JoinSet<PingState>,
    next_probe: Instant,
    addresses: Arc<Mutex<AddressCache>>,
}

impl ServerPing {
    fn new(now: Instant) -> Self {
        Self {
            state: PingState::Checking,
            task: JoinSet::new(),
            next_probe: now,
            addresses: Arc::default(),
        }
    }

    fn update(&mut self, address: &str, now: Instant) {
        if let Some(result) = self.task.try_join_next() {
            self.state = result.unwrap_or(PingState::Unreachable);
        }
        if self.task.is_empty() && now >= self.next_probe {
            let address = address.to_owned();
            let addresses = Arc::clone(&self.addresses);
            self.task.spawn(async move { probe(&address, &addresses).await });
            self.next_probe = now + REFRESH_INTERVAL;
        }
    }
}

#[derive(Default)]
pub struct ServerPings {
    entries: HashMap<String, ServerPing>,
}

impl ServerPings {
    pub fn update(&mut self, servers: &[MpServer]) {
        // Dropping a JoinSet cancels its probe, including any open client.
        self.entries.retain(|address, _| servers.iter().any(|server| server.address == *address));
        let now = Instant::now();
        for server in servers {
            if let Some(entry) = self.entries.get_mut(&server.address) {
                entry.update(&server.address, now);
            } else {
                let mut entry = ServerPing::new(now);
                entry.update(&server.address, now);
                self.entries.insert(server.address.clone(), entry);
            }
        }
    }

    pub fn state(&self, address: &str) -> PingState {
        self.entries.get(address).map_or(PingState::Checking, |entry| entry.state)
    }
}

#[cfg(test)]
mod tests {
    use super::{probe, PingState, ServerPing, ServerPings, REFRESH_INTERVAL};
    use futures_util::FutureExt;
    use phira_mp_common::{ClientCommand, ServerCommand, Stream};
    use prpr::config::MpServer;
    use tokio::{
        io::AsyncReadExt,
        net::TcpListener,
        runtime::Builder,
        sync::{mpsc, oneshot, Mutex},
        time::{advance, pause, timeout, Duration, Instant},
    };

    #[test]
    fn measures_protocol_ping_without_authentication() {
        Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap().to_string();
            let (seen_tx, mut seen_rx) = mpsc::unbounded_channel();
            let (stop_tx, stop_rx) = oneshot::channel::<()>();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let _stream = Stream::<ServerCommand, ClientCommand>::new(
                    None,
                    stream,
                    Box::new(move |sender, command| {
                        let seen_tx = seen_tx.clone();
                        async move {
                            assert!(matches!(command, ClientCommand::Ping));
                            sender.send(ServerCommand::Pong).await.unwrap();
                            seen_tx.send(()).unwrap();
                        }
                    }),
                )
                .await
                .unwrap();
                let _ = stop_rx.await;
            });
            assert!(matches!(probe(&address, &Mutex::default()).await, PingState::Ready(_)));
            seen_rx.try_recv().unwrap();
            stop_tx.send(()).unwrap();
            server.await.unwrap();
        });
    }

    #[test]
    fn retries_a_timeout_once_and_uses_the_retry_result() {
        Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for retry_responds in [true, false] {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap().to_string();
                let task = tokio::spawn(async move { probe(&address, &Mutex::default()).await });
                let (mut first, _) = timeout(Duration::from_secs(1), listener.accept()).await.unwrap().unwrap();
                let mut version = [0];
                first.read_exact(&mut version).await.unwrap();
                pause();
                advance(Duration::from_secs(2)).await;
                tokio::time::resume();

                let (second, _) = timeout(Duration::from_secs(1), listener.accept()).await.unwrap().unwrap();
                if retry_responds {
                    let _stream = Stream::<ServerCommand, ClientCommand>::new(
                        None,
                        second,
                        Box::new(|sender, command| async move {
                            assert!(matches!(command, ClientCommand::Ping));
                            sender.send(ServerCommand::Pong).await.unwrap();
                        }),
                    )
                    .await
                    .unwrap();
                    assert!(matches!(task.await.unwrap(), PingState::Ready(_)));
                } else {
                    let mut second = second;
                    second.read_exact(&mut version).await.unwrap();
                    pause();
                    advance(Duration::from_secs(2)).await;
                    assert!(matches!(task.await.unwrap(), PingState::TimedOut));
                    tokio::time::resume();
                }
                assert!(listener.accept().now_or_never().is_none());
            }
        });
    }

    #[test]
    fn refreshes_all_servers_after_ten_seconds_without_overlapping_probes() {
        Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            pause();
            let servers = [
                MpServer {
                    name: "First".to_owned(),
                    address: "first.invalid:12345".to_owned(),
                },
                MpServer {
                    name: "Second".to_owned(),
                    address: "second.invalid:12345".to_owned(),
                },
            ];
            let mut pings = ServerPings::default();
            for server in &servers {
                let mut entry = ServerPing::new(Instant::now());
                entry.next_probe += REFRESH_INTERVAL;
                entry.task.spawn(async { PingState::Ready(Duration::from_millis(40)) });
                pings.entries.insert(server.address.clone(), entry);
            }

            tokio::task::yield_now().await;
            advance(Duration::from_secs(9)).await;
            pings.update(&servers);
            assert!(pings.entries.values().all(|entry| entry.task.is_empty()));
            assert!(pings.entries.values().all(|entry| matches!(entry.state, PingState::Ready(_))));

            advance(Duration::from_secs(1)).await;
            pings.update(&servers);
            assert!(pings.entries.values().all(|entry| entry.task.len() == 1));
            pings.update(&servers);
            assert!(pings.entries.values().all(|entry| entry.task.len() == 1));
            assert!(pings.entries.values().all(|entry| matches!(entry.state, PingState::Ready(_))));
            // Cancel before yielding: this test exercises scheduling without DNS or TCP.
            drop(pings);
        });
    }

    #[test]
    fn removing_server_or_leaving_page_closes_in_flight_connection() {
        Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            for leave_page in [false, true] {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap().to_string();
                let mut pings = ServerPings::default();
                pings.update(&[MpServer {
                    name: "Test".to_owned(),
                    address,
                }]);
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut version = [0];
                stream.read_exact(&mut version).await.unwrap();
                if leave_page {
                    drop(pings);
                } else {
                    pings.update(&[]);
                }
                let mut pending_bytes = Vec::new();
                timeout(Duration::from_secs(1), stream.read_to_end(&mut pending_bytes)).await.unwrap().unwrap();
            }
        });
    }
}
