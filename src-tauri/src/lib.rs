pub mod audio;
pub mod hotkey;
pub mod models;
pub mod server;
pub mod storage;

use audio::AudioEngine;
use models::{AppConfig, AudioDeviceInfo, NetworkInfo, SoundItem};
use server::RemoteServer;
use std::sync::{Arc, Mutex};
use tauri::State;
use uuid::Uuid;

pub struct AppState {
    pub audio_engine: AudioEngine,
    pub server: RemoteServer,
    pub sounds: Arc<Mutex<Vec<SoundItem>>>,
    pub theme: Arc<Mutex<String>>,
    pub hotkey_manager: hotkey::HotkeyManager,
}

fn save_state_config(state: &AppState) {
    let sounds = state.sounds.lock().unwrap().clone();
    let master_volume = state.audio_engine.get_master_volume();
    let server_port = state.server.get_port();
    let primary_device = state.audio_engine.get_primary_device();
    let secondary_device = state.audio_engine.get_secondary_device();
    let theme = Some(state.theme.lock().unwrap().clone());
    let remote_password = state.server.get_password();

    let config = AppConfig {
        sounds,
        master_volume,
        server_port,
        primary_device,
        secondary_device,
        theme,
        remote_password,
    };

    if let Err(e) = storage::save_config(&config) {
        eprintln!("[OpenSB] Error saving config: {}", e);
    }
}

#[tauri::command]
fn get_app_config(state: State<'_, AppState>) -> AppConfig {
    let sounds = {
        let s = state.sounds.lock().unwrap();
        let mut list = s.clone();
        for item in list.iter_mut() {
            item.is_playing = state.audio_engine.is_playing(&item.id);
        }
        list
    };
    AppConfig {
        sounds,
        master_volume: state.audio_engine.get_master_volume(),
        server_port: state.server.get_port(),
        primary_device: state.audio_engine.get_primary_device(),
        secondary_device: state.audio_engine.get_secondary_device(),
        theme: Some(state.theme.lock().unwrap().clone()),
        remote_password: state.server.get_password(),
    }
}

#[tauri::command]
fn set_theme(state: State<'_, AppState>, theme: String) -> Result<(), String> {
    *state.theme.lock().unwrap() = theme;
    save_state_config(&state);
    Ok(())
}

#[tauri::command]
fn get_remote_password(state: State<'_, AppState>) -> Option<String> {
    state.server.get_password()
}

#[tauri::command]
fn set_remote_password(state: State<'_, AppState>, password: Option<String>) -> Result<(), String> {
    state.server.set_password(password);
    save_state_config(&state);
    Ok(())
}

#[tauri::command]
fn get_sounds(state: State<'_, AppState>) -> Vec<SoundItem> {
    let sounds = state.sounds.lock().unwrap();
    let mut result = sounds.clone();
    for s in result.iter_mut() {
        s.is_playing = state.audio_engine.is_playing(&s.id);
    }
    result
}

#[tauri::command]
fn add_sound(
    state: State<'_, AppState>,
    title: String,
    path: String,
    hotkey: Option<String>,
    color: Option<String>,
    play_mode: Option<String>,
    volume: Option<f32>,
) -> Result<SoundItem, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Audio file not found at path: {}", path));
    }

    let id = format!("snd_{}", Uuid::new_v4().simple());
    let new_sound = SoundItem {
        id: id.clone(),
        title,
        path,
        hotkey,
        color: color.or_else(|| Some("#3B82F6".to_string())),
        category: Some("General".to_string()),
        duration_ms: 0,
        is_playing: false,
        volume: volume.unwrap_or(1.0).clamp(0.0, 1.0),
        play_mode: play_mode.unwrap_or_else(|| "oneshot".to_string()),
    };

    {
        let mut sounds = state.sounds.lock().unwrap();
        sounds.push(new_sound.clone());
    }

    save_state_config(&state);

    // Sync to remote Android clients
    state.server.broadcast_sync();

    Ok(new_sound)
}

#[tauri::command]
fn update_sound(
    state: State<'_, AppState>,
    id: String,
    title: String,
    hotkey: Option<String>,
    color: Option<String>,
    play_mode: String,
    volume: f32,
    path: Option<String>,
) -> Result<SoundItem, String> {
    let mut sounds = state.sounds.lock().unwrap();
    if let Some(s) = sounds.iter_mut().find(|item| item.id == id) {
        s.title = title;
        s.hotkey = hotkey;
        s.color = color;
        s.play_mode = play_mode;
        s.volume = volume.clamp(0.0, 1.0);
        if let Some(new_path) = path {
            if !new_path.trim().is_empty() {
                s.path = new_path;
            }
        }
        let updated = s.clone();
        drop(sounds);
        save_state_config(&state);
        state.server.broadcast_sync();
        Ok(updated)
    } else {
        Err(format!("Sound ID '{}' not found", id))
    }
}

#[tauri::command]
fn remove_sound(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.audio_engine.stop(&id);
    {
        let mut sounds = state.sounds.lock().unwrap();
        sounds.retain(|s| s.id != id);
    }
    save_state_config(&state);
    state.server.broadcast_sync();
    Ok(())
}

#[tauri::command]
fn play_sound(state: State<'_, AppState>, id: String, action: Option<String>) -> Result<(), String> {
    let sound = {
        let sounds = state.sounds.lock().unwrap();
        sounds.iter().find(|s| s.id == id).cloned()
    };

    if let Some(s) = sound {
        let act = action.unwrap_or_else(|| "PLAY".to_string());
        if act == "STOP" || act == "HOLD_UP" {
            state.audio_engine.stop(&s.id);
            state.server.broadcast_audio_state(&s.id, false);
        } else if act == "TOGGLE" && state.audio_engine.is_playing(&s.id) {
            state.audio_engine.stop(&s.id);
            state.server.broadcast_audio_state(&s.id, false);
        } else {
            let mode_to_use = if act == "HOLD_DOWN" { "hold" } else { &s.play_mode };
            state.audio_engine.play(&s.id, &s.path, s.volume, mode_to_use)?;
            state.server.broadcast_audio_state(&s.id, true);
        }
        Ok(())
    } else {
        Err(format!("Sound ID '{}' not found", id))
    }
}

#[tauri::command]
fn stop_sound(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state.audio_engine.stop(&id);
    state.server.broadcast_audio_state(&id, false);
    Ok(())
}

#[tauri::command]
fn stop_all(state: State<'_, AppState>) -> Result<(), String> {
    state.audio_engine.stop_all();
    let sounds = state.sounds.lock().unwrap().clone();
    for s in sounds {
        state.server.broadcast_audio_state(&s.id, false);
    }
    Ok(())
}

#[tauri::command]
fn set_master_volume(state: State<'_, AppState>, volume: f32) -> Result<(), String> {
    state.audio_engine.set_master_volume(volume);
    save_state_config(&state);
    state.server.broadcast_sync();
    Ok(())
}

#[tauri::command]
fn get_network_info(state: State<'_, AppState>) -> NetworkInfo {
    NetworkInfo {
        ip: state.server.get_local_ip(),
        port: state.server.get_port(),
        connected_clients: state.server.client_count(),
    }
}

#[tauri::command]
fn set_server_port(state: State<'_, AppState>, port: u16) -> Result<NetworkInfo, String> {
    state.server.set_port(port)?;
    save_state_config(&state);
    Ok(NetworkInfo {
        ip: state.server.get_local_ip(),
        port: state.server.get_port(),
        connected_clients: state.server.client_count(),
    })
}

#[tauri::command]
fn get_audio_devices() -> Vec<AudioDeviceInfo> {
    AudioEngine::list_devices()
}

#[tauri::command]
fn set_audio_devices(
    state: State<'_, AppState>,
    primary: Option<String>,
    secondary: Option<String>,
) -> Result<(), String> {
    state.audio_engine.set_devices(primary, secondary);
    save_state_config(&state);
    Ok(())
}

#[tauri::command]
fn set_hotkeys_enabled(state: State<'_, AppState>, enabled: bool) {
    state.hotkey_manager.set_enabled(enabled);
}

#[tauri::command]
fn set_app_focused(state: State<'_, AppState>, focused: bool) {
    state.hotkey_manager.set_app_focused(focused);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let loaded_config = storage::load_config();

    let audio_engine = AudioEngine::new();
    audio_engine.set_master_volume(loaded_config.master_volume);
    if loaded_config.primary_device.is_some() || loaded_config.secondary_device.is_some() {
        audio_engine.set_devices(
            loaded_config.primary_device.clone(),
            loaded_config.secondary_device.clone(),
        );
    }

    let sounds = Arc::new(Mutex::new(loaded_config.sounds));
    let server = RemoteServer::new(sounds.clone(), audio_engine.clone());
    let _ = server.set_port(loaded_config.server_port);
    server.set_password(loaded_config.remote_password);

    let theme = Arc::new(Mutex::new(
        loaded_config.theme.unwrap_or_else(|| "dark".to_string()),
    ));

    // Auto-notify remote clients when audio finishes playing
    let server_notify = server.clone();
    audio_engine.set_on_sound_stopped(move |sound_id| {
        server_notify.broadcast_audio_state(&sound_id, false);
    });

    // Start WebSocket & UDP Discovery background threads
    server.start();

    // Start global pass-through keyboard hook
    let hotkey_manager = hotkey::HotkeyManager::new(sounds.clone(), audio_engine.clone(), server.clone());

    let app_state = AppState {
        audio_engine,
        server,
        sounds,
        theme,
        hotkey_manager,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_app_config,
            set_theme,
            get_remote_password,
            set_remote_password,
            get_sounds,
            add_sound,
            update_sound,
            remove_sound,
            play_sound,
            stop_sound,
            stop_all,
            set_master_volume,
            get_network_info,
            set_server_port,
            get_audio_devices,
            set_audio_devices,
            set_hotkeys_enabled,
            set_app_focused
        ])
        .run(tauri::generate_context!())
        .expect("error while running OpenSoundBoard application");
}
