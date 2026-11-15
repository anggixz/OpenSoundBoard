use crate::audio::AudioEngine;
use crate::models::{AudioStatePayload, SoundItem, SyncPayload};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::mpsc;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;

pub type ClientSender = mpsc::UnboundedSender<String>;

#[derive(Clone)]
pub struct RemoteServer {
    clients: Arc<Mutex<Vec<ClientSender>>>,
    sounds_provider: Arc<Mutex<Vec<SoundItem>>>,
    audio_engine: AudioEngine,
    local_ip: Arc<Mutex<String>>,
    port: Arc<Mutex<u16>>,
    port_tx: Arc<Mutex<Option<mpsc::UnboundedSender<u16>>>>,
    password: Arc<Mutex<Option<String>>>,
}

impl RemoteServer {
    pub fn new(sounds: Arc<Mutex<Vec<SoundItem>>>, audio_engine: AudioEngine) -> Self {
        let ip = match local_ip_address::local_ip() {
            Ok(ip) => ip.to_string(),
            Err(_) => "127.0.0.1".to_string(),
        };

        Self {
            clients: Arc::new(Mutex::new(Vec::new())),
            sounds_provider: sounds,
            audio_engine,
            local_ip: Arc::new(Mutex::new(ip)),
            port: Arc::new(Mutex::new(58900)),
            port_tx: Arc::new(Mutex::new(None)),
            password: Arc::new(Mutex::new(None)),
        }
    }

    pub fn get_local_ip(&self) -> String {
        self.local_ip.lock().unwrap().clone()
    }

    pub fn get_port(&self) -> u16 {
        *self.port.lock().unwrap()
    }

    pub fn get_password(&self) -> Option<String> {
        self.password.lock().unwrap().clone()
    }

    pub fn set_password(&self, pass: Option<String>) {
        let cleaned = pass.and_then(|p| {
            let trimmed = p.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        *self.password.lock().unwrap() = cleaned;
    }

    pub fn set_port(&self, new_port: u16) -> Result<(), String> {
        if new_port < 1024 {
            return Err("Port must be between 1024 and 65535".to_string());
        }
        // Test binding to port to verify it's not already occupied by another application
        match std::net::TcpListener::bind(format!("0.0.0.0:{}", new_port)) {
            Ok(test_listener) => {
                drop(test_listener);
            }
            Err(e) => {
                return Err(format!("Port {} is unavailable (it may already be in use by another application): {}", new_port, e));
            }
        }

        *self.port.lock().unwrap() = new_port;
        if let Some(ref tx) = *self.port_tx.lock().unwrap() {
            let _ = tx.send(new_port);
        }
        Ok(())
    }

    pub fn client_count(&self) -> usize {
        self.clients.lock().unwrap().len()
    }

    pub fn broadcast(&self, message: &str) {
        let mut clients = self.clients.lock().unwrap();
        clients.retain(|sender| sender.send(message.to_string()).is_ok());
    }

    pub fn broadcast_sync(&self) {
        let sounds = self.sounds_provider.lock().unwrap().clone();
        let master_vol = self.audio_engine.get_master_volume();
        let payload = SyncPayload {
            sounds,
            master_volume: master_vol,
            primary_device: None,
            secondary_device: None,
        };

        let msg = serde_json::json!({
            "type": "SYNC_SOUNDS",
            "payload": payload
        });

        self.broadcast(&msg.to_string());
    }

    pub fn broadcast_audio_state(&self, sound_id: &str, is_playing: bool) {
        let payload = AudioStatePayload {
            sound_id: sound_id.to_string(),
            state: if is_playing { "PLAYING".to_string() } else { "STOPPED".to_string() },
            elapsed_ms: 0,
        };

        let msg = serde_json::json!({
            "type": "AUDIO_STATE_CHANGED",
            "payload": payload
        });

        self.broadcast(&msg.to_string());
    }

    pub fn start(&self) {
        let this = self.clone();

        // Spawn Tokio Async Runtime for WebSocket Server & UDP Discovery
        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime for OpenSB remote server");

            rt.block_on(async move {
                let (port_tx, mut port_rx) = mpsc::unbounded_channel::<u16>();
                {
                    let mut tx_guard = this.port_tx.lock().unwrap();
                    *tx_guard = Some(port_tx);
                }

                let mut current_port = *this.port.lock().unwrap();

                loop {
                    // 1. Spawn UDP Discovery Broadcaster with current_port & auth status
                    let udp_ip = this.get_local_ip();
                    let udp_port = current_port;
                    let udp_auth_required = this.get_password().is_some();
                    let udp_abort = Arc::new(tokio::sync::Notify::new());
                    let udp_abort_clone = udp_abort.clone();
                    let udp_task = tokio::spawn(async move {
                        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
                            let _ = socket.set_broadcast(true);
                            let broadcast_addr: SocketAddr = "255.255.255.255:58901".parse().unwrap();
                            let msg = serde_json::json!({
                                "service": "OpenSoundBoard",
                                "version": "1.0.0",
                                "port": udp_port,
                                "ip": udp_ip,
                                "name": "OpenSoundBoard Desktop",
                                "authRequired": udp_auth_required
                            })
                            .to_string();

                            loop {
                                tokio::select! {
                                    _ = socket.send_to(msg.as_bytes(), broadcast_addr) => {},
                                    _ = udp_abort_clone.notified() => break,
                                }
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                                    _ = udp_abort_clone.notified() => break,
                                }
                            }
                        }
                    });

                    // 2. Start WebSocket TCP Listener on current_port
                    let bind_addr = format!("0.0.0.0:{}", current_port);
                    let listener = match TcpListener::bind(&bind_addr).await {
                        Ok(l) => {
                            println!("[OpenSB] Remote WebSocket Server running on {}", bind_addr);
                            Some(l)
                        }
                        Err(e) => {
                            eprintln!("[OpenSB] Failed to bind WebSocket server port {}: {:?}", current_port, e);
                            None
                        }
                    };

                    let next_port = if let Some(listener) = listener {
                        loop {
                            tokio::select! {
                                accept_res = listener.accept() => {
                                    if let Ok((stream, addr)) = accept_res {
                                        let server_clone = this.clone();
                                        tokio::spawn(async move {
                                            println!("[OpenSB] New remote client connected from: {}", addr);
                                            if let Ok(ws_stream) = accept_async(stream).await {
                                                server_clone.handle_connection(ws_stream).await;
                                            }
                                        });
                                    }
                                }
                                Some(new_p) = port_rx.recv() => {
                                    println!("[OpenSB] Switching WebSocket server port from {} to {}", current_port, new_p);
                                    break new_p;
                                }
                            }
                        }
                    } else {
                        match port_rx.recv().await {
                            Some(new_p) => new_p,
                            None => break,
                        }
                    };

                    udp_abort.notify_waiters();
                    let _ = udp_task.await;
                    {
                        let mut clients = this.clients.lock().unwrap();
                        clients.clear();
                    }
                    current_port = next_port;
                }
            });
        });
    }

    async fn handle_connection<S>(&self, ws_stream: tokio_tungstenite::WebSocketStream<S>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (mut write, mut read) = ws_stream.split();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();

        let requires_password = self.get_password().is_some();
        let mut is_authenticated = !requires_password;

        if is_authenticated {
            // Register client immediately if no password required
            {
                let mut clients = self.clients.lock().unwrap();
                clients.push(tx.clone());
            }

            // Send initial state sync to this new client
            let initial_sync = {
                let sounds = self.sounds_provider.lock().unwrap().clone();
                let master_vol = self.audio_engine.get_master_volume();
                serde_json::json!({
                    "type": "SYNC_SOUNDS",
                    "payload": SyncPayload {
                        sounds,
                        master_volume: master_vol,
                        primary_device: None,
                        secondary_device: None,
                    }
                })
                .to_string()
            };
            let _ = write.send(Message::Text(initial_sync.into())).await;
        } else {
            // Password required, notify client to authenticate
            let auth_req = serde_json::json!({
                "type": "AUTH_REQUIRED",
                "payload": {
                    "message": "Password required to connect to OpenSoundBoard"
                }
            })
            .to_string();
            let _ = write.send(Message::Text(auth_req.into())).await;
        }

        // Task to forward outgoing messages to WebSocket
        let forward_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if write.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });

        // Loop to receive incoming messages from Android
        let this = self.clone();
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    let v: Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    let msg_type = match v.get("type").and_then(|t| t.as_str()) {
                        Some(t) => t,
                        None => continue,
                    };

                    let payload = v.get("payload");

                    // Handle authentication request
                    if msg_type == "AUTH" {
                        let provided = payload
                            .and_then(|p| p.get("password"))
                            .and_then(|s| s.as_str())
                            .unwrap_or("")
                            .trim();

                        let expected = this.get_password().unwrap_or_default();
                        if !requires_password || provided == expected {
                            if !is_authenticated {
                                is_authenticated = true;
                                {
                                    let mut clients = this.clients.lock().unwrap();
                                    clients.push(tx.clone());
                                }
                            }

                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "AUTH_SUCCESS",
                                    "payload": {
                                        "message": "Authentication successful"
                                    }
                                })
                                .to_string(),
                            );

                            let initial_sync = {
                                let sounds = this.sounds_provider.lock().unwrap().clone();
                                let master_vol = this.audio_engine.get_master_volume();
                                serde_json::json!({
                                    "type": "SYNC_SOUNDS",
                                    "payload": SyncPayload {
                                        sounds,
                                        master_volume: master_vol,
                                        primary_device: None,
                                        secondary_device: None,
                                    }
                                })
                                .to_string()
                            };
                            let _ = tx.send(initial_sync);
                        } else {
                            let _ = tx.send(
                                serde_json::json!({
                                    "type": "AUTH_ERROR",
                                    "payload": {
                                        "message": "Incorrect password"
                                    }
                                })
                                .to_string(),
                            );
                        }
                        continue;
                    }

                    // For any commands other than AUTH, reject if not authenticated
                    if !is_authenticated {
                        let _ = tx.send(
                            serde_json::json!({
                                "type": "AUTH_ERROR",
                                "payload": {
                                    "message": "Access denied: password required"
                                }
                            })
                            .to_string(),
                        );
                        continue;
                    }

                    this.handle_client_message(&text);
                }
                Ok(Message::Close(_)) => break,
                Err(_) => break,
                _ => {}
            }
        }

        forward_task.abort();
    }

    fn handle_client_message(&self, text: &str) {
        let v: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        let msg_type = match v.get("type").and_then(|t| t.as_str()) {
            Some(t) => t,
            None => return,
        };

        let payload = v.get("payload");

        match msg_type {
            "TRIGGER_SOUND" => {
                if let Some(sound_id) = payload.and_then(|p| p.get("soundId")).and_then(|s| s.as_str()) {
                    let action = payload
                        .and_then(|p| p.get("action"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("PLAY");

                    let sound_item = {
                        let sounds = self.sounds_provider.lock().unwrap();
                        sounds.iter().find(|s| s.id == sound_id).cloned()
                    };

                    if let Some(sound) = sound_item {
                        if action == "STOP" || action == "HOLD_UP" {
                            self.audio_engine.stop(&sound.id);
                            self.broadcast_audio_state(&sound.id, false);
                        } else if action == "TOGGLE" && self.audio_engine.is_playing(&sound.id) {
                            self.audio_engine.stop(&sound.id);
                            self.broadcast_audio_state(&sound.id, false);
                        } else {
                            // "PLAY", "HOLD_DOWN", or re-trigger
                            let mode_to_use = if action == "HOLD_DOWN" { "hold" } else { &sound.play_mode };
                            if let Ok(_) = self.audio_engine.play(&sound.id, &sound.path, sound.volume, mode_to_use) {
                                self.broadcast_audio_state(&sound.id, true);
                            }
                        }
                    }
                }
            }
            "STOP_ALL" => {
                self.audio_engine.stop_all();
                let sounds = self.sounds_provider.lock().unwrap().clone();
                for s in sounds {
                    self.broadcast_audio_state(&s.id, false);
                }
            }
            "SET_VOLUME" => {
                if let Some(vol) = payload.and_then(|p| p.get("volume")).and_then(|v| v.as_f64()) {
                    self.audio_engine.set_master_volume(vol as f32);
                    self.broadcast_sync();
                }
            }
            _ => {}
        }
    }
}
