use std::{path::PathBuf, sync::Arc, time::Duration};

use activity_protocol::ActivityEvent;
use anyhow::{Context, Result};
use async_trait::async_trait;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub const IPC_PROTOCOL: &str = "agent-activity-ipc/1.0";
const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub enum Endpoint {
    #[cfg(unix)]
    Unix(PathBuf),
    #[cfg(windows)]
    NamedPipe(String),
}

impl Endpoint {
    pub fn current_user() -> Result<Self> {
        let dirs = ProjectDirs::from("work", "Effective Work", "Agent Activity Hub")
            .context("cannot resolve application data directory")?;
        #[cfg(unix)]
        {
            Ok(Self::Unix(dirs.data_local_dir().join("ipc-v1.sock")))
        }
        #[cfg(windows)]
        {
            let user = std::env::var("USERNAME").unwrap_or_else(|_| "current-user".into());
            let safe_user: String = user
                .chars()
                .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
                .collect();
            let _ = dirs;
            Ok(Self::NamedPipe(format!(
                r"\\.\pipe\agent-activity-v1-{safe_user}"
            )))
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    Hello {
        protocol: String,
        client: String,
        nonce: String,
    },
    Emit {
        event: Box<ActivityEvent>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub accepted: bool,
    pub code: String,
}

impl Response {
    pub fn ok(code: impl Into<String>) -> Self {
        Self {
            accepted: true,
            code: code.into(),
        }
    }

    pub fn error(code: impl Into<String>) -> Self {
        Self {
            accepted: false,
            code: code.into(),
        }
    }
}

#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    async fn handle(&self, event: ActivityEvent) -> Response;
}

pub async fn send_event(
    endpoint: &Endpoint,
    event: ActivityEvent,
    timeout: Duration,
) -> Result<Response> {
    tokio::time::timeout(timeout, send_event_inner(endpoint, event))
        .await
        .context("IPC timeout")?
}

#[cfg(unix)]
async fn send_event_inner(endpoint: &Endpoint, event: ActivityEvent) -> Result<Response> {
    use tokio::net::UnixStream;

    let Endpoint::Unix(path) = endpoint;
    let mut stream = UnixStream::connect(path)
        .await
        .with_context(|| format!("connect to {}", path.display()))?;
    write_request(
        &mut stream,
        &Request::Hello {
            protocol: IPC_PROTOCOL.into(),
            client: "activity-hook".into(),
            nonce: event.event_id.clone(),
        },
    )
    .await?;
    let hello = read_response(&mut stream).await?;
    anyhow::ensure!(hello.accepted, "IPC handshake rejected: {}", hello.code);
    write_request(
        &mut stream,
        &Request::Emit {
            event: Box::new(event),
        },
    )
    .await?;
    read_response(&mut stream).await
}

#[cfg(windows)]
async fn send_event_inner(endpoint: &Endpoint, event: ActivityEvent) -> Result<Response> {
    use tokio::net::windows::named_pipe::ClientOptions;

    let Endpoint::NamedPipe(path) = endpoint;
    let mut stream = ClientOptions::new()
        .open(path)
        .with_context(|| format!("connect to {path}"))?;
    write_request(
        &mut stream,
        &Request::Hello {
            protocol: IPC_PROTOCOL.into(),
            client: "activity-hook".into(),
            nonce: event.event_id.clone(),
        },
    )
    .await?;
    let hello = read_response(&mut stream).await?;
    anyhow::ensure!(hello.accepted, "IPC handshake rejected: {}", hello.code);
    write_request(
        &mut stream,
        &Request::Emit {
            event: Box::new(event),
        },
    )
    .await?;
    read_response(&mut stream).await
}

#[cfg(unix)]
pub async fn serve(endpoint: Endpoint, handler: Arc<dyn EventHandler>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::{UnixListener, UnixStream};

    let Endpoint::Unix(path) = endpoint;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        if UnixStream::connect(&path).await.is_ok() {
            anyhow::bail!("IPC endpoint already has a live owner: {}", path.display());
        }
        let _ = tokio::fs::remove_file(&path).await;
    }
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    loop {
        let (stream, _) = listener.accept().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            let _ = serve_connection(stream, handler).await;
        });
    }
}

#[cfg(windows)]
pub async fn serve(endpoint: Endpoint, handler: Arc<dyn EventHandler>) -> Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let Endpoint::NamedPipe(path) = endpoint;
    let mut first = true;
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(first)
            .create(&path)?;
        first = false;
        server.connect().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            let _ = serve_connection(server, handler).await;
        });
    }
}

async fn serve_connection<S>(stream: S, handler: Arc<dyn EventHandler>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    let hello = read_request_line(&mut lines).await?;
    match hello {
        Request::Hello { protocol, .. } if protocol == IPC_PROTOCOL => {
            write_response(lines.get_mut(), &Response::ok("ready")).await?;
        }
        _ => {
            write_response(lines.get_mut(), &Response::error("protocol_mismatch")).await?;
            return Ok(());
        }
    }
    let request = read_request_line(&mut lines).await?;
    let response = match request {
        Request::Emit { event } => handler.handle(*event).await,
        Request::Hello { .. } => Response::error("unexpected_hello"),
    };
    write_response(lines.get_mut(), &response).await
}

async fn read_request_line<R>(lines: &mut tokio::io::Lines<BufReader<R>>) -> Result<Request>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let line = lines.next_line().await?.context("IPC peer closed")?;
    anyhow::ensure!(line.len() <= MAX_MESSAGE_BYTES, "IPC message too large");
    serde_json::from_str(&line).context("invalid IPC request")
}

async fn write_request<W>(writer: &mut W, request: &Request) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(request)?;
    anyhow::ensure!(payload.len() <= MAX_MESSAGE_BYTES, "IPC message too large");
    writer.write_all(&payload).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

async fn read_response<R>(reader: &mut R) -> Result<Response>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut line = String::new();
    let mut reader = BufReader::new(reader);
    reader.read_line(&mut line).await?;
    anyhow::ensure!(line.len() <= MAX_MESSAGE_BYTES, "IPC response too large");
    serde_json::from_str(&line).context("invalid IPC response")
}

async fn write_response<W>(writer: &mut W, response: &Response) -> Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(response)?;
    writer.write_all(&payload).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AcceptingHandler;

    #[async_trait]
    impl EventHandler for AcceptingHandler {
        async fn handle(&self, _event: ActivityEvent) -> Response {
            Response::ok("accepted")
        }
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unix_ipc_round_trip_requires_successful_handshake() {
        let path = std::env::temp_dir().join(format!(
            "aah-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
                % 1_000_000_000
        ));
        let endpoint = Endpoint::Unix(path.clone());
        let server_endpoint = endpoint.clone();
        let server =
            tokio::spawn(async move { serve(server_endpoint, Arc::new(AcceptingHandler)).await });

        let mut response = None;
        let mut last_error = None;
        for _ in 0..20 {
            match send_event(&endpoint, test_event(), Duration::from_secs(1)).await {
                Ok(value) => {
                    response = Some(value);
                    break;
                }
                Err(error) => {
                    last_error = Some(format!("{error:#}"));
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }

        if server.is_finished() {
            panic!(
                "IPC server exited early: {:?}; last client error: {last_error:?}",
                server.await
            );
        }
        assert!(
            response.is_some_and(|value| value.accepted),
            "last IPC error: {last_error:?}"
        );
        server.abort();
        let _ = tokio::fs::remove_file(path).await;
    }

    fn test_event() -> ActivityEvent {
        serde_json::from_value(serde_json::json!({
            "schema_version": "1.0",
            "event_id": "ipc-test-event",
            "provider": "codex",
            "adapter_id": "builtin.codex",
            "adapter_version": "0.1.0",
            "source_kind": "native_hook",
            "instance_id": "local",
            "session_id": "session-1",
            "kind": "model.working",
            "occurred_at": "2026-07-15T00:00:00Z",
            "observed_at": "2026-07-15T00:00:00Z",
            "attributes": {}
        }))
        .unwrap()
    }
}
