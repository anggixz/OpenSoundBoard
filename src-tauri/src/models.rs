use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundItem {
    pub id: String,
    pub title: String,
    pub path: String,
    pub hotkey: Option<String>,
    pub color: Option<String>,
    pub category: Option<String>,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    #[serde(rename = "isPlaying")]
    pub is_playing: bool,
    pub volume: f32,
    #[serde(rename = "playMode")]
    pub play_mode: String, // "oneshot" | "toggle" | "hold" | "overlap"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPayload {
    pub sounds: Vec<SoundItem>,
    #[serde(rename = "masterVolume")]
    pub master_volume: f32,
    #[serde(rename = "primaryDevice")]
    pub primary_device: Option<String>,
    #[serde(rename = "secondaryDevice")]
    pub secondary_device: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStatePayload {
    #[serde(rename = "soundId")]
    pub sound_id: String,
    pub state: String, // "PLAYING" | "STOPPED"
    #[serde(rename = "elapsedMs")]
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInfo {
    pub ip: String,
    pub port: u16,
    pub connected_clients: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub sounds: Vec<SoundItem>,
    #[serde(rename = "masterVolume")]
    pub master_volume: f32,
    #[serde(rename = "serverPort")]
    pub server_port: u16,
    #[serde(rename = "primaryDevice")]
    pub primary_device: Option<String>,
    #[serde(rename = "secondaryDevice")]
    pub secondary_device: Option<String>,
    pub theme: Option<String>,
    #[serde(rename = "remotePassword")]
    pub remote_password: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            sounds: Vec::new(),
            master_volume: 1.0,
            server_port: 58900,
            primary_device: None,
            secondary_device: None,
            theme: Some("dark".to_string()),
            remote_password: None,
        }
    }
}
