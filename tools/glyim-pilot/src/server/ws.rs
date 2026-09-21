use crate::error::PilotError;
use crate::server::messages::ExtensionMessage;
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;

const EVENT_CHANNEL_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
/// ServerEvent.
pub enum ServerEvent {
    /// Variant.
    Connected {
        /// Struct.
        addr: SocketAddr,
    },
    /// Variant.
    Message {
        /// Struct.
        session_id: Option<String>,
        /// Struct.
        trace_id: Option<String>,
        /// Struct.
        msg: ExtensionMessage,
    },
    /// Variant.
    Disconnected {
        /// Struct.
        addr: SocketAddr,
    },
}

/// WsServer.
pub struct WsServer {
    addr: SocketAddr,
    event_tx: mpsc::Sender<ServerEvent>,
    event_rx: Option<mpsc::Receiver<ServerEvent>>,
    cli_msg_tx: broadcast::Sender<String>,
}

impl WsServer {
    /// new.
    pub fn new(host: &str, port: u16) -> Self {
        let addr: SocketAddr = format!("{host}:{port}")
            .parse()
            .expect("invalid bind address");
        let (event_tx, event_rx) = mpsc::channel(EVENT_CHANNEL_CAPACITY);
        let (cli_msg_tx, _) = broadcast::channel(256);
        Self {
            addr,
            event_tx,
            event_rx: Some(event_rx),
            cli_msg_tx,
        }
    }
    /// take_event_rx.
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<ServerEvent>> {
        self.event_rx.take()
    }
    /// cli_msg_sender.
    pub fn cli_msg_sender(&self) -> broadcast::Sender<String> {
        self.cli_msg_tx.clone()
    }
    /// run.
    pub async fn run(&self) -> Result<(), PilotError> {
        let listener = TcpListener::bind(&self.addr).await?;
        tracing::info!("WebSocket server listening on ws://{}", self.addr);
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    if !addr.ip().is_loopback() {
                        tracing::error!(peer = %addr, "REJECTED non-localhost connection");
                        continue;
                    }
                    let event_tx = self.event_tx.clone();
                    let cli_msg_tx = self.cli_msg_tx.clone();
                    tokio::spawn(async move {
                        let ws_stream = match tokio_tungstenite::accept_async(stream).await {
                            Ok(ws) => ws,
                            Err(e) => {
                                tracing::warn!(peer = %addr, "handshake failed: {e}");
                                return;
                            }
                        };
                        let _ = event_tx.send(ServerEvent::Connected { addr }).await;
                        let (ws_sender, mut ws_receiver) = ws_stream.split();
                        let sender = Arc::new(Mutex::new(ws_sender));

                        // Spawn task to forward CLI messages (broadcast) to the WebSocket
                        let sender_cli = sender.clone();
                        let mut cli_rx = cli_msg_tx.subscribe();
                        let send_task = tokio::spawn(async move {
                            while let Ok(msg) = cli_rx.recv().await {
                                let mut guard = sender_cli.lock().await;
                                if guard.send(Message::Text(msg.into())).await.is_err() {
                                    break;
                                }
                            }
                        });

                        // Main loop: handle incoming messages and pings
                        while let Some(msg) = ws_receiver.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    if let Ok(ext_msg) =
                                        serde_json::from_str::<ExtensionMessage>(&text)
                                    {
                                        let sid = ext_msg.session_id().map(|s| s.to_string());
                                        let tid = ext_msg.trace_id().map(|s| s.to_string());
                                        let _ = event_tx
                                            .send(ServerEvent::Message {
                                                session_id: sid,
                                                trace_id: tid,
                                                msg: ext_msg,
                                            })
                                            .await;
                                    }
                                }
                                Ok(Message::Ping(data)) => {
                                    // Send pong using the shared sender
                                    let mut guard = sender.lock().await;
                                    let _ = guard.send(Message::Pong(data)).await;
                                }
                                Ok(Message::Close(_)) => break,
                                _ => {}
                            }
                        }
                        send_task.abort();
                        let _ = event_tx.send(ServerEvent::Disconnected { addr }).await;
                    });
                }
                Err(e) => {
                    tracing::error!("accept failed: {e}");
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
        }
    }
}
