use crate::models::AppConfig;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;

pub fn get_config_path() -> PathBuf {
    // 1. Check if we can use portable mode (config.json next to executable)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let local_config = exe_dir.join("config.json");
            if local_config.exists() {
                return local_config;
            }

            // Check if executable is NOT in Program Files (installer directory)
            let exe_str = exe_path.to_string_lossy().to_lowercase();
            let is_in_program_files = exe_str.contains("program files");
            if !is_in_program_files {
                let test_file = exe_dir.join(".write_test");
                if fs::write(&test_file, b"test").is_ok() {
                    let _ = fs::remove_file(test_file);
                    return local_config;
                }
            }
        }
    }

    // 2. Fallback to AppData/Roaming/OpenSoundBoard/config.json (Standard Windows Installer location)
    let app_data = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    let app_dir = app_data.join("OpenSoundBoard");
    let _ = fs::create_dir_all(&app_dir);
    app_dir.join("config.json")
}

pub fn load_config() -> AppConfig {
    let path = get_config_path();
    if path.exists() {
        if let Ok(file) = File::open(&path) {
            let reader = BufReader::new(file);
            if let Ok(mut config) = serde_json::from_reader::<_, AppConfig>(reader) {
                // Ensure all sounds are initialized with is_playing = false
                for s in config.sounds.iter_mut() {
                    s.is_playing = false;
                }
                println!("[OpenSB] Loaded {} sound(s) from {:?}", config.sounds.len(), path);
                return config;
            }
        }
    }
    println!("[OpenSB] No existing config found at {:?}, starting with fresh defaults", path);
    AppConfig::default()
}

pub fn save_config(config: &AppConfig) -> Result<(), String> {
    let path = get_config_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let file = File::create(&path)
        .map_err(|e| format!("Failed to create configuration file at {:?}: {}", path, e))?;
    let writer = BufWriter::new(file);
    serde_json::to_writer_pretty(writer, config)
        .map_err(|e| format!("Failed to write configuration to JSON: {}", e))?;
    Ok(())
}
