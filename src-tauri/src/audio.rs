use crate::models::AudioDeviceInfo;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{cpal, Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub enum AudioCommand {
    Play {
        sound_id: String,
        file_path: String,
        volume: f32,
        play_mode: String,
    },
    Stop {
        sound_id: String,
    },
    StopAll,
    SetMasterVolume(f32),
    SetDevices {
        primary_name: Option<String>,
        secondary_name: Option<String>,
    },
}

pub type SoundStoppedCallback = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct AudioEngine {
    tx: Arc<Mutex<Sender<AudioCommand>>>,
    master_volume: Arc<Mutex<f32>>,
    playing_states: Arc<Mutex<HashMap<String, bool>>>,
    primary_device_name: Arc<Mutex<Option<String>>>,
    secondary_device_name: Arc<Mutex<Option<String>>>,
    on_sound_stopped: Arc<Mutex<Option<SoundStoppedCallback>>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        let (tx, rx) = channel::<AudioCommand>();
        let playing_states = Arc::new(Mutex::new(HashMap::new()));
        let playing_states_thread = playing_states.clone();

        let primary_device_name = Arc::new(Mutex::new(None));
        let secondary_device_name = Arc::new(Mutex::new(None));

        let on_sound_stopped: Arc<Mutex<Option<SoundStoppedCallback>>> = Arc::new(Mutex::new(None));
        let on_sound_stopped_thread = on_sound_stopped.clone();

        thread::spawn(move || {
            // Function to setup output stream from optional device name
            let setup_stream = |device_name: Option<&str>| -> Option<(OutputStream, OutputStreamHandle)> {
                match device_name {
                    Some("NONE") => None,
                    Some(target_name) if !target_name.trim().is_empty() => {
                        let host = cpal::default_host();
                        if let Ok(devices) = host.output_devices() {
                            for dev in devices {
                                if let Ok(name) = dev.name() {
                                    if name.trim().eq_ignore_ascii_case(target_name.trim()) {
                                        if let Ok(res) = OutputStream::try_from_device(&dev) {
                                            return Some(res);
                                        }
                                    }
                                }
                            }
                        }
                        eprintln!("[OpenSB Audio] Output device '{}' not found or failed to initialize", target_name);
                        None
                    }
                    _ => OutputStream::try_default().ok(),
                }
            };

            let mut primary_stream_pair = setup_stream(None);
            let mut secondary_stream_pair: Option<(OutputStream, OutputStreamHandle)> = None;

            // Map of sound_id -> Vec of Sinks (for primary and secondary outputs)
            let mut sinks: HashMap<String, Vec<Sink>> = HashMap::new();
            let mut master_vol: f32 = 1.0;

            loop {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(cmd) => match cmd {
                        AudioCommand::Play {
                            sound_id,
                            file_path,
                            volume,
                            play_mode,
                        } => {
                            let is_already_playing = sinks
                                .get(&sound_id)
                                .map(|list| list.iter().any(|s| !s.empty()))
                                .unwrap_or(false);

                            // Handle Play Modes:
                            // 1. "toggle": If already playing, stop and do not restart.
                            if play_mode == "toggle" && is_already_playing {
                                if let Some(list) = sinks.get_mut(&sound_id) {
                                    for s in list.iter() {
                                        s.stop();
                                    }
                                    list.clear();
                                }
                                let mut states = playing_states_thread.lock().unwrap();
                                states.insert(sound_id.clone(), false);
                                if let Some(ref cb) = *on_sound_stopped_thread.lock().unwrap() {
                                    cb(sound_id);
                                }
                                continue;
                            }

                            // 2. "hold": If already playing, keep looping continuously without stutter/restart!
                            if play_mode == "hold" && is_already_playing {
                                continue;
                            }

                            // 3. "oneshot": If already playing, stop previous sink so it restarts clean from start.
                            if play_mode == "oneshot" && is_already_playing {
                                if let Some(list) = sinks.get_mut(&sound_id) {
                                    for s in list.iter() {
                                        s.stop();
                                    }
                                    list.clear();
                                }
                            }

                            // 4. "overlap": Multiple instances play concurrently without stopping previous.

                            let target_vol = volume * master_vol;
                            let mut new_sinks = Vec::new();

                            // Play on Primary Output (Monitoring / Headphones)
                            if let Some((_, ref handle)) = primary_stream_pair {
                                if let Ok(file) = File::open(&file_path) {
                                    if let Ok(source) = Decoder::new(BufReader::new(file)) {
                                        if let Ok(sink) = Sink::try_new(handle) {
                                            sink.set_volume(target_vol);
                                            if play_mode == "hold" {
                                                sink.append(source.repeat_infinite());
                                            } else {
                                                sink.append(source);
                                            }
                                            new_sinks.push(sink);
                                        }
                                    }
                                }
                            }

                            // Play on Secondary Output (Broadcast / Virtual Cable / Mic)
                            if let Some((_, ref handle)) = secondary_stream_pair {
                                if let Ok(file) = File::open(&file_path) {
                                    if let Ok(source) = Decoder::new(BufReader::new(file)) {
                                        if let Ok(sink) = Sink::try_new(handle) {
                                            sink.set_volume(target_vol);
                                            if play_mode == "hold" {
                                                sink.append(source.repeat_infinite());
                                            } else {
                                                sink.append(source);
                                            }
                                            new_sinks.push(sink);
                                        }
                                    }
                                }
                            }

                            if !new_sinks.is_empty() {
                                let list = sinks.entry(sound_id.clone()).or_default();
                                if play_mode != "overlap" {
                                    list.retain(|s| !s.empty());
                                }
                                list.extend(new_sinks);

                                let mut states = playing_states_thread.lock().unwrap();
                                states.insert(sound_id, true);
                            }
                        }
                        AudioCommand::Stop { sound_id } => {
                            if let Some(list) = sinks.get_mut(&sound_id) {
                                for s in list.iter() {
                                    s.stop();
                                }
                                list.clear();
                            }
                            let mut states = playing_states_thread.lock().unwrap();
                            states.insert(sound_id.clone(), false);
                            if let Some(ref cb) = *on_sound_stopped_thread.lock().unwrap() {
                                cb(sound_id);
                            }
                        }
                        AudioCommand::StopAll => {
                            for (_, list) in sinks.iter_mut() {
                                for s in list.iter() {
                                    s.stop();
                                }
                                list.clear();
                            }
                            let mut states = playing_states_thread.lock().unwrap();
                            for (id, val) in states.iter_mut() {
                                if *val {
                                    *val = false;
                                    if let Some(ref cb) = *on_sound_stopped_thread.lock().unwrap() {
                                        cb(id.clone());
                                    }
                                }
                            }
                        }
                        AudioCommand::SetMasterVolume(v) => {
                            master_vol = v.clamp(0.0, 1.0);
                        }
                        AudioCommand::SetDevices {
                            primary_name,
                            secondary_name,
                        } => {
                            primary_stream_pair = setup_stream(primary_name.as_deref());
                            secondary_stream_pair = setup_stream(secondary_name.as_deref());
                        }
                    },
                    Err(RecvTimeoutError::Timeout) => {
                        // Periodic watchdog tick
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }

                // Auto-cleanup finished audio sinks and reset playing status to stop!
                let mut finished_sounds = Vec::new();
                for (sound_id, list) in sinks.iter_mut() {
                    list.retain(|s| !s.empty());
                    if list.is_empty() {
                        let mut states = playing_states_thread.lock().unwrap();
                        if let Some(is_playing) = states.get_mut(sound_id) {
                            if *is_playing {
                                *is_playing = false;
                                finished_sounds.push(sound_id.clone());
                            }
                        }
                    }
                }

                if !finished_sounds.is_empty() {
                    if let Some(ref cb) = *on_sound_stopped_thread.lock().unwrap() {
                        for id in finished_sounds {
                            cb(id);
                        }
                    }
                }
            }
        });

        Self {
            tx: Arc::new(Mutex::new(tx)),
            master_volume: Arc::new(Mutex::new(1.0)),
            playing_states,
            primary_device_name,
            secondary_device_name,
            on_sound_stopped,
        }
    }

    pub fn set_on_sound_stopped<F>(&self, callback: F)
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        let mut guard = self.on_sound_stopped.lock().unwrap();
        *guard = Some(Arc::new(callback));
    }

    pub fn play(&self, sound_id: &str, file_path: &str, volume: f32, play_mode: &str) -> Result<(), String> {
        let sender = self.tx.lock().unwrap();
        sender
            .send(AudioCommand::Play {
                sound_id: sound_id.to_string(),
                file_path: file_path.to_string(),
                volume,
                play_mode: play_mode.to_string(),
            })
            .map_err(|e| format!("Failed to dispatch audio play: {:?}", e))
    }

    pub fn stop(&self, sound_id: &str) {
        let sender = self.tx.lock().unwrap();
        let _ = sender.send(AudioCommand::Stop {
            sound_id: sound_id.to_string(),
        });
    }

    pub fn stop_all(&self) {
        let sender = self.tx.lock().unwrap();
        let _ = sender.send(AudioCommand::StopAll);
    }

    pub fn is_playing(&self, sound_id: &str) -> bool {
        let states = self.playing_states.lock().unwrap();
        *states.get(sound_id).unwrap_or(&false)
    }

    pub fn set_master_volume(&self, volume: f32) {
        let mut mv = self.master_volume.lock().unwrap();
        *mv = volume.clamp(0.0, 1.0);
        let sender = self.tx.lock().unwrap();
        let _ = sender.send(AudioCommand::SetMasterVolume(*mv));
    }

    pub fn get_master_volume(&self) -> f32 {
        *self.master_volume.lock().unwrap()
    }

    pub fn set_devices(&self, primary: Option<String>, secondary: Option<String>) {
        {
            let mut p = self.primary_device_name.lock().unwrap();
            *p = primary.clone();
            let mut s = self.secondary_device_name.lock().unwrap();
            *s = secondary.clone();
        }
        let sender = self.tx.lock().unwrap();
        let _ = sender.send(AudioCommand::SetDevices {
            primary_name: primary,
            secondary_name: secondary,
        });
    }

    pub fn get_primary_device(&self) -> Option<String> {
        self.primary_device_name.lock().unwrap().clone()
    }

    pub fn get_secondary_device(&self) -> Option<String> {
        self.secondary_device_name.lock().unwrap().clone()
    }

    pub fn list_devices() -> Vec<AudioDeviceInfo> {
        let host = cpal::default_host();
        let default_device_name = host.default_output_device().and_then(|d| d.name().ok());
        let mut list = Vec::new();

        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if let Ok(name) = dev.name() {
                    let is_def = default_device_name.as_ref().map(|d| d == &name).unwrap_or(false);
                    list.push(AudioDeviceInfo {
                        name,
                        is_default: is_def,
                    });
                }
            }
        }
        list
    }
}
