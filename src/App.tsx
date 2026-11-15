import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Volume2,
  VolumeX,
  Play,
  Square,
  Plus,
  Trash2,
  Wifi,
  Sun,
  Moon,
  FolderOpen,
  Keyboard,
  FileAudio,
  Radio,
  Settings,
  Headphones,
  Mic,
  Lock,
  Eye,
  EyeOff,
  X,
} from "lucide-react";
import "./App.css";

interface SoundItem {
  id: string;
  title: string;
  path: string;
  hotkey?: string;
  color?: string;
  category?: string;
  durationMs: number;
  isPlaying: boolean;
  volume: number;
  playMode: "oneshot" | "toggle" | "hold" | "overlap";
}

interface NetworkInfo {
  ip: string;
  port: number;
  connected_clients: number;
}

interface AudioDevice {
  name: string;
  is_default: boolean;
}

interface AppConfig {
  sounds: SoundItem[];
  masterVolume: number;
  serverPort: number;
  primaryDevice?: string;
  secondaryDevice?: string;
  theme?: "dark" | "light";
  remotePassword?: string | null;
}

export default function App() {
  const [sounds, setSounds] = useState<SoundItem[]>([]);
  const [masterVolume, setMasterVolume] = useState<number>(1.0);
  const [networkInfo, setNetworkInfo] = useState<NetworkInfo>({
    ip: "127.0.0.1",
    port: 58900,
    connected_clients: 0,
  });
  const [theme, setTheme] = useState<"dark" | "light">(() => {
    return (localStorage.getItem("opensb_theme") as "dark" | "light") || "dark";
  });

  // Audio devices state
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [primaryDevice, setPrimaryDevice] = useState<string>("NONE");
  const [secondaryDevice, setSecondaryDevice] = useState<string>("NONE");
  const [formPrimaryDevice, setFormPrimaryDevice] = useState<string>("NONE");
  const [formSecondaryDevice, setFormSecondaryDevice] = useState<string>("NONE");
  const [isAudioSettingsOpen, setIsAudioSettingsOpen] = useState(false);

  // Network / Port / Security Settings state
  const [isNetworkModalOpen, setIsNetworkModalOpen] = useState(false);
  const [formPort, setFormPort] = useState<string>("58900");
  const [remotePassword, setRemotePassword] = useState<string>("");
  const [showPassword, setShowPassword] = useState<boolean>(false);
  const [portError, setPortError] = useState<string>("");

  // Modal dialog states (Add / Edit)
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [editingSoundId, setEditingSoundId] = useState<string | null>(null);
  const [formTitle, setFormTitle] = useState("");
  const [formPath, setFormPath] = useState("");
  const [formHotkey, setFormHotkey] = useState("");
  const [formPlayMode, setFormPlayMode] = useState<"oneshot" | "toggle" | "hold" | "overlap">("oneshot");
  const [formVolume, setFormVolume] = useState<number>(1.0);

  // Hotkey Recorder interactive state
  const [isRecordingHotkey, setIsRecordingHotkey] = useState(false);
  const [recordingPreview, setRecordingPreview] = useState("");
  const recorderRef = useRef<HTMLDivElement>(null);

  // Sync theme
  useEffect(() => {
    document.documentElement.setAttribute("data-theme", theme);
    localStorage.setItem("opensb_theme", theme);
  }, [theme]);

  const toggleTheme = () => {
    const nextTheme = theme === "dark" ? "light" : "dark";
    setTheme(nextTheme);
    invoke("set_theme", { theme: nextTheme }).catch(console.error);
  };

  // Fetch sounds and system info (Polls every 250ms for instant status updates)
  const loadData = useCallback(async () => {
    try {
      const soundsList = await invoke<SoundItem[]>("get_sounds");
      setSounds(soundsList);
      const net = await invoke<NetworkInfo>("get_network_info");
      setNetworkInfo(net);
    } catch (e) {
      console.error("[OpenSB] Error loading data:", e);
    }
  }, []);

  // One-time initialization on app mount
  useEffect(() => {
    let isMounted = true;
    async function initApp() {
      try {
        const config = await invoke<AppConfig>("get_app_config");
        if (!isMounted) return;
        setSounds(config.sounds || []);
        setMasterVolume(config.masterVolume ?? 1.0);
        if (config.serverPort) {
          setFormPort(config.serverPort.toString());
        }
        if (config.remotePassword !== undefined && config.remotePassword !== null) {
          setRemotePassword(config.remotePassword);
        }
        if (config.theme && (config.theme === "dark" || config.theme === "light")) {
          setTheme(config.theme);
        }

        const devices = await invoke<AudioDevice[]>("get_audio_devices");
        if (!isMounted) return;
        setAudioDevices(devices);

        // Load primary device from config
        if (config.primaryDevice) {
          setPrimaryDevice(config.primaryDevice);
        } else {
          // If never set before, use default device
          const def = devices.find((d) => d.is_default);
          if (def) {
            setPrimaryDevice(def.name);
          } else if (devices.length > 0) {
            setPrimaryDevice(devices[0].name);
          } else {
            setPrimaryDevice("NONE");
          }
        }

        // Load secondary device from config
        if (config.secondaryDevice) {
          setSecondaryDevice(config.secondaryDevice);
        } else {
          setSecondaryDevice("NONE");
        }
      } catch (e) {
        console.error("[OpenSB] Error initializing app:", e);
      }
      loadData();
    }

    initApp();

    const interval = setInterval(loadData, 250);
    return () => {
      isMounted = false;
      clearInterval(interval);
    };
  }, [loadData]);

  // Handle Audio Actions
  const handleTrigger = async (
    sound: SoundItem,
    action: "PLAY" | "STOP" | "TOGGLE" | "HOLD_DOWN" | "HOLD_UP"
  ) => {
    try {
      await invoke("play_sound", { id: sound.id, action });
      loadData();
    } catch (e) {
      console.error("[OpenSB] Error triggering sound:", e);
    }
  };

  const handleStopAll = async () => {
    try {
      await invoke("stop_all");
      setSounds((prev) => prev.map((s) => ({ ...s, isPlaying: false })));
    } catch (e) {
      console.error("[OpenSB] Error stopping all:", e);
    }
  };

  const handleMasterVolumeChange = async (val: number) => {
    setMasterVolume(val);
    try {
      await invoke("set_master_volume", { volume: val });
    } catch (e) {
      console.error("[OpenSB] Error setting master volume:", e);
    }
  };

  const handleDeleteSound = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await invoke("remove_sound", { id });
      loadData();
    } catch (e) {
      console.error("[OpenSB] Error removing sound:", e);
    }
  };

  const handleOpenEdit = (sound: SoundItem, e: React.MouseEvent) => {
    e.stopPropagation();
    setEditingSoundId(sound.id);
    setFormTitle(sound.title);
    setFormPath(sound.path);
    setFormHotkey(sound.hotkey || "");
    setFormPlayMode(sound.playMode || "oneshot");
    setFormVolume(sound.volume ?? 1.0);
    setIsRecordingHotkey(false);
    setRecordingPreview("");
    setIsModalOpen(true);
  };

  const handleOpenAdd = () => {
    setEditingSoundId(null);
    setFormTitle("");
    setFormPath("");
    setFormHotkey("");
    setFormPlayMode("oneshot");
    setFormVolume(1.0);
    setIsRecordingHotkey(false);
    setRecordingPreview("");
    setIsModalOpen(true);
  };

  // Pick file via Tauri Native Dialog
  const handleBrowseFile = async () => {
    try {
      const selected = await open({
        multiple: false,
        filters: [
          {
            name: "Audio Files",
            extensions: ["mp3", "wav", "ogg", "flac"],
          },
        ],
      });

      if (selected && typeof selected === "string") {
        setFormPath(selected);
        if (!formTitle) {
          const filename = selected.split(/\\|\//).pop() || "";
          const nameWithoutExt = filename.replace(/\.[^/.]+$/, "");
          setFormTitle(nameWithoutExt);
        }
      }
    } catch (e) {
      console.error("[OpenSB] Error opening file dialog:", e);
    }
  };

  const handleSaveSound = async () => {
    if (!formPath || !formTitle) return;

    try {
      if (editingSoundId) {
        await invoke("update_sound", {
          id: editingSoundId,
          title: formTitle.trim(),
          hotkey: formHotkey.trim() ? formHotkey.trim().toUpperCase() : null,
          color: "#3B82F6",
          playMode: formPlayMode,
          volume: formVolume,
          path: formPath.trim() || null,
        });
      } else {
        await invoke("add_sound", {
          title: formTitle.trim(),
          path: formPath.trim(),
          hotkey: formHotkey.trim() ? formHotkey.trim().toUpperCase() : null,
          color: "#3B82F6",
          playMode: formPlayMode,
          volume: formVolume,
        });
      }
      setIsModalOpen(false);
      loadData();
    } catch (e) {
      alert(`Failed to save audio: ${e}`);
    }
  };

  const handleOpenAudioSettings = async () => {
    try {
      const devices = await invoke<AudioDevice[]>("get_audio_devices");
      setAudioDevices(devices);
    } catch (e) {
      console.error("[OpenSB] Error refreshing audio devices:", e);
    }
    setFormPrimaryDevice(primaryDevice || "NONE");
    setFormSecondaryDevice(secondaryDevice || "NONE");
    setIsAudioSettingsOpen(true);
  };

  const handleSaveAudioDevices = async () => {
    try {
      const p = formPrimaryDevice.trim() ? formPrimaryDevice.trim() : "NONE";
      const s = formSecondaryDevice.trim() ? formSecondaryDevice.trim() : "NONE";
      await invoke("set_audio_devices", {
        primary: p,
        secondary: s,
      });
      setPrimaryDevice(p);
      setSecondaryDevice(s);
      setIsAudioSettingsOpen(false);
    } catch (e) {
      alert(`Failed to save audio devices: ${e}`);
    }
  };

  const handleSaveNetworkSettings = async () => {
    const portNum = parseInt(formPort.trim(), 10);
    if (isNaN(portNum) || portNum < 1024 || portNum > 65535) {
      setPortError("Port number must be between 1024 and 65535.");
      return;
    }

    try {
      const updated = await invoke<NetworkInfo>("set_server_port", { port: portNum });
      setNetworkInfo(updated);
      await invoke("set_remote_password", {
        password: remotePassword.trim() ? remotePassword.trim() : null,
      });
      setIsNetworkModalOpen(false);
      setPortError("");
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as Error)?.message || "Failed to save server settings.";
      setPortError(msg);
    }
  };

  const handleResetDefaultPort = async () => {
    try {
      const updated = await invoke<NetworkInfo>("set_server_port", { port: 58900 });
      setNetworkInfo(updated);
      setFormPort("58900");
      setRemotePassword("");
      await invoke("set_remote_password", { password: null });
      setIsNetworkModalOpen(false);
      setPortError("");
    } catch (e: unknown) {
      const msg = typeof e === "string" ? e : (e as Error)?.message || "Failed to reset server settings.";
      setPortError(msg);
    }
  };

  // Interactive Hotkey Recorder Keydown Listener
  const handleRecordKeyDown = (e: React.KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      setIsRecordingHotkey(false);
      setRecordingPreview("");
      return;
    }

    if (e.key === "Backspace" || e.key === "Delete") {
      setFormHotkey("");
      setIsRecordingHotkey(false);
      setRecordingPreview("");
      return;
    }

    const modifiers: string[] = [];
    if (e.ctrlKey) modifiers.push("CTRL");
    if (e.altKey) modifiers.push("ALT");
    if (e.shiftKey) modifiers.push("SHIFT");
    if (e.metaKey) modifiers.push("WIN");

    const isModifierOnly = ["Control", "Alt", "Shift", "Meta"].includes(e.key);

    if (isModifierOnly) {
      setRecordingPreview(modifiers.join(" + ") + " + ...");
      return;
    }

    // Determine normalized key name
    let keyName = e.key.toUpperCase();
    if (e.code.startsWith("Digit")) {
      keyName = e.code.replace("Digit", "");
    } else if (e.code.startsWith("Key")) {
      keyName = e.code.replace("Key", "");
    } else if (e.code.startsWith("Numpad")) {
      keyName = "NUM" + e.code.replace("Numpad", "");
    } else if (e.key === " ") {
      keyName = "SPACE";
    }

    const finalCombination = [...modifiers, keyName].join(" + ");
    setFormHotkey(finalCombination);
    setIsRecordingHotkey(false);
    setRecordingPreview("");
  };

  // Focus recorder box when recording starts
  useEffect(() => {
    if (isRecordingHotkey && recorderRef.current) {
      recorderRef.current.focus();
    }
  }, [isRecordingHotkey]);

  // Keep live reference for focused window key listeners
  const soundsRef = useRef(sounds);
  useEffect(() => {
    soundsRef.current = sounds;
  }, [sounds]);

  const activeHoldKeysRef = useRef<Set<string>>(new Set());

  // Pause global hotkeys whenever any modal or hotkey recording is active
  useEffect(() => {
    const isAnyModalOrRecordingOpen =
      isModalOpen || isNetworkModalOpen || isAudioSettingsOpen || isRecordingHotkey;
    invoke("set_hotkeys_enabled", { enabled: !isAnyModalOrRecordingOpen }).catch((err) =>
      console.warn("[OpenSB] Error updating hotkeys enabled state:", err)
    );
  }, [isModalOpen, isNetworkModalOpen, isAudioSettingsOpen, isRecordingHotkey]);

  // Match pressed key with recorded hotkey combination (e.g. CTRL + 1, CTRL + ALT + A, 1, A, +)
  const isHotkeyMatched = useCallback((e: KeyboardEvent, hotkeyStr: string): boolean => {
    if (!hotkeyStr) return false;
    const trimmed = hotkeyStr.trim().toUpperCase();
    let parts: string[];
    let mainKey: string;
    let modifiers: string[];

    if (trimmed === "+") {
      mainKey = "+";
      modifiers = [];
    } else if (trimmed.endsWith("+")) {
      parts = trimmed.split("+").map((p) => p.trim()).filter(Boolean);
      mainKey = "+";
      modifiers = parts;
    } else {
      parts = trimmed.split("+").map((p) => p.trim()).filter(Boolean);
      if (parts.length === 0) return false;
      mainKey = parts[parts.length - 1];
      modifiers = parts.slice(0, -1);
    }

    const ctrlRequired = modifiers.includes("CTRL") || modifiers.includes("CONTROL");
    const altRequired = modifiers.includes("ALT") || modifiers.includes("OPTION");
    const shiftRequired = modifiers.includes("SHIFT");
    const winRequired = modifiers.includes("WIN") || modifiers.includes("SUPER") || modifiers.includes("META");

    if (e.ctrlKey !== ctrlRequired) return false;
    if (e.altKey !== altRequired) return false;
    if (e.shiftKey !== shiftRequired) return false;
    if (e.metaKey !== winRequired) return false;

    let pressedKey = e.key.toUpperCase();
    if (e.code.startsWith("Digit")) {
      pressedKey = e.code.replace("Digit", "");
    } else if (e.code.startsWith("Key")) {
      pressedKey = e.code.replace("Key", "");
    } else if (e.code.startsWith("Numpad")) {
      pressedKey = "NUM" + e.code.replace("Numpad", "");
    } else if (e.key === " ") {
      pressedKey = "SPACE";
    }

    return pressedKey === mainKey;
  }, []);

  // In-app focused keydown & keyup handlers with ZERO debounce for instant responsive spamming
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Don't trigger if user is typing in an input field or recording hotkey
      if (
        document.activeElement?.tagName === "INPUT" ||
        document.activeElement?.tagName === "TEXTAREA" ||
        document.activeElement?.tagName === "SELECT" ||
        isRecordingHotkey
      ) {
        return;
      }

      if (e.key === "Escape") {
        handleStopAll();
        return;
      }

      const matched = soundsRef.current.find((s) => s.hotkey && isHotkeyMatched(e, s.hotkey));
      if (matched) {
        if (matched.playMode === "hold") {
          if (!activeHoldKeysRef.current.has(matched.id)) {
            activeHoldKeysRef.current.add(matched.id);
            handleTrigger(matched, "HOLD_DOWN");
          }
        } else if (matched.playMode === "toggle") {
          if (!e.repeat) {
            handleTrigger(matched, "TOGGLE");
          }
        } else {
          // One-shot and overlap: zero debounce, spamming allowed!
          handleTrigger(matched, "PLAY");
        }
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (isRecordingHotkey) return;

      const matched = soundsRef.current.find((s) => s.hotkey && isHotkeyMatched(e, s.hotkey));
      if (matched && matched.playMode === "hold") {
        activeHoldKeysRef.current.delete(matched.id);
        handleTrigger(matched, "HOLD_UP");
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
    };
  }, [isRecordingHotkey, isHotkeyMatched]);

  return (
    <div className="app-container">
      {/* Top Navbar */}
      <header className="app-header">
        <div className="brand-section">
          <div className="logo-badge">
            <Radio size={20} />
          </div>
          <div>
            <h1 className="brand-title">OpenSoundBoard</h1>
            <p className="brand-tagline">High Performance Sound Deck & Remote</p>
          </div>
        </div>

        <div className="header-actions">
          {/* Dual Audio Routing Button */}
          <button
            className="icon-btn"
            onClick={handleOpenAudioSettings}
            title="Dual Audio Routing Settings (Speaker + Virtual Cable)"
          >
            <Headphones size={15} />
            <span>Dual Output</span>
          </button>

          {/* Remote Network Status & Port / Security Setting */}
          <button
            className="network-pill network-pill-btn"
            onClick={() => {
              setFormPort(networkInfo.port.toString());
              setPortError("");
              setIsNetworkModalOpen(true);
            }}
            title="Click to configure Remote Server Port & Security (Wi-Fi)"
          >
            <Wifi size={14} />
            <span>
              {networkInfo.ip}:{networkInfo.port} ({networkInfo.connected_clients} {networkInfo.connected_clients === 1 ? "Remote" : "Remotes"})
            </span>
            {remotePassword.trim() && (
              <span title="Password Protected" style={{ display: "inline-flex", alignItems: "center", marginLeft: "2px" }}>
                <Lock size={12} />
              </span>
            )}
          </button>

          {/* Theme Toggle Button */}
          <button
            className="icon-btn"
            onClick={toggleTheme}
            title={theme === "dark" ? "Switch to Light Mode" : "Switch to Dark Mode"}
          >
            {theme === "dark" ? <Sun size={17} /> : <Moon size={17} />}
          </button>
        </div>
      </header>

      {/* Control Actions & Master Volume */}
      <section className="controls-bar">
        <div className="volume-wrapper">
          {masterVolume === 0 ? <VolumeX size={18} /> : <Volume2 size={18} />}
          <input
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={masterVolume}
            onChange={(e) => handleMasterVolumeChange(parseFloat(e.target.value))}
            className="volume-slider"
          />
          <span className="volume-label">{Math.round(masterVolume * 100)}%</span>
        </div>

        <div className="btn-group">
          <button className="btn-primary" onClick={handleOpenAdd}>
            <Plus size={16} />
            Add Sound
          </button>

          <button className="btn-danger" onClick={handleStopAll} title="Press ESC to Stop All Immediately">
            <Square size={16} />
            STOP ALL (ESC)
          </button>
        </div>
      </section>

      {/* Main Sound Grid */}
      <main className="main-content">
        {sounds.length === 0 ? (
          <div className="empty-state">
            <FileAudio size={48} className="empty-icon" />
            <h2 className="empty-title">No Sounds Added Yet</h2>
            <p className="empty-desc">
              Add your favorite audio files (.mp3, .wav, .ogg, .flac) with flexible playback modes (One-shot, Toggle, Hold to Play, Overlap) controllable via custom hotkeys like CTRL+1, CTRL+ALT+A, or an Android remote client.
            </p>
            <button className="btn-primary" onClick={handleOpenAdd}>
              <Plus size={16} />
              Add First Sound
            </button>
          </div>
        ) : (
          <div className="sound-grid">
            {sounds.map((sound) => {
              const modeLabel =
                sound.playMode === "toggle"
                  ? "TOGGLE"
                  : sound.playMode === "hold"
                  ? "HOLD"
                  : sound.playMode === "overlap"
                  ? "OVERLAP"
                  : "1-SHOT";

              return (
                <div
                  key={sound.id}
                  className={`sound-tile ${sound.isPlaying ? "playing" : ""}`}
                  onMouseDown={() => {
                    if (sound.playMode === "hold") {
                      handleTrigger(sound, "HOLD_DOWN");
                    }
                  }}
                  onMouseUp={() => {
                    if (sound.playMode === "hold") {
                      handleTrigger(sound, "HOLD_UP");
                    }
                  }}
                  onMouseLeave={() => {
                    if (sound.playMode === "hold" && sound.isPlaying) {
                      handleTrigger(sound, "HOLD_UP");
                    }
                  }}
                  onClick={() => {
                    if (sound.playMode === "toggle") {
                      handleTrigger(sound, "TOGGLE");
                    } else if (sound.playMode !== "hold") {
                      handleTrigger(sound, "PLAY");
                    }
                  }}
                >
                  <div className="tile-top">
                    <div className="tile-icon">
                      {sound.isPlaying ? <Volume2 size={18} /> : <Play size={18} />}
                    </div>
                    <div className="tile-badges">
                      <span className="mode-badge">{modeLabel}</span>
                      {sound.hotkey && <span className="hotkey-badge">{sound.hotkey}</span>}
                    </div>
                  </div>

                  <div className="tile-title" title={sound.title}>
                    {sound.title}
                  </div>

                  <div className="tile-bottom">
                    <span className="tile-status-text">
                      {sound.isPlaying ? "Playing..." : `${Math.round((sound.volume ?? 1) * 100)}%`}
                    </span>
                    <div className="tile-actions">
                      {/* Settings Icon for Edit */}
                      <button
                        className="action-icon-btn"
                        onMouseDown={(e) => e.stopPropagation()}
                        onMouseUp={(e) => e.stopPropagation()}
                        onClick={(e) => handleOpenEdit(sound, e)}
                        title="Edit Sound Settings"
                      >
                        <Settings size={14} />
                      </button>
                      {sound.isPlaying && (
                        <button
                           className="action-icon-btn"
                          onMouseDown={(e) => e.stopPropagation()}
                          onMouseUp={(e) => e.stopPropagation()}
                          onClick={(e) => {
                            e.stopPropagation();
                            handleTrigger(sound, "STOP");
                          }}
                          title="Stop Sound"
                        >
                          <Square size={13} />
                        </button>
                      )}
                      <button
                        className="action-icon-btn delete"
                        onMouseDown={(e) => e.stopPropagation()}
                        onMouseUp={(e) => e.stopPropagation()}
                        onClick={(e) => handleDeleteSound(sound.id, e)}
                        title="Delete Sound"
                      >
                        <Trash2 size={13} />
                      </button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </main>

      {/* Modal Add / Edit Sound */}
      {isModalOpen && (
        <div className="modal-overlay" onClick={() => setIsModalOpen(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="modal-title">
                {editingSoundId ? "Edit Soundboard Sound" : "Add Soundboard Sound"}
              </h3>
            </div>

            <div className="form-group">
              <label className="form-label">Audio File (*.mp3, *.wav, *.ogg, *.flac)</label>
              <div style={{ display: "flex", gap: "8px" }}>
                <input
                  type="text"
                  className="form-input"
                  placeholder="Select audio file..."
                  value={formPath}
                  onChange={(e) => setFormPath(e.target.value)}
                />
                <button
                  className="btn-secondary"
                  onClick={handleBrowseFile}
                  type="button"
                  title="Browse Audio File"
                >
                  <FolderOpen size={16} />
                </button>
              </div>
            </div>

            <div className="form-group">
              <label className="form-label">Button Title</label>
              <input
                type="text"
                className="form-input"
                placeholder="e.g. Airhorn, Applause, Wow"
                value={formTitle}
                onChange={(e) => setFormTitle(e.target.value)}
              />
            </div>

            {/* Playback Mode */}
            <div className="form-group">
              <label className="form-label">Playback Mode</label>
              <div className="mode-selector">
                <div
                  className={`mode-option-btn ${formPlayMode === "oneshot" ? "selected" : ""}`}
                  onClick={() => setFormPlayMode("oneshot")}
                >
                  <span className="mode-option-title">One-shot (Restart)</span>
                  <span className="mode-option-desc">Triggering again restarts audio from the beginning</span>
                </div>

                <div
                  className={`mode-option-btn ${formPlayMode === "toggle" ? "selected" : ""}`}
                  onClick={() => setFormPlayMode("toggle")}
                >
                  <span className="mode-option-title">Toggle (Play / Stop)</span>
                  <span className="mode-option-desc">Trigger once to play, trigger again to stop</span>
                </div>

                <div
                  className={`mode-option-btn ${formPlayMode === "hold" ? "selected" : ""}`}
                  onClick={() => setFormPlayMode("hold")}
                >
                  <span className="mode-option-title">Hold to Play</span>
                  <span className="mode-option-desc">Plays while held down, stops immediately on release</span>
                </div>

                <div
                  className={`mode-option-btn ${formPlayMode === "overlap" ? "selected" : ""}`}
                  onClick={() => setFormPlayMode("overlap")}
                >
                  <span className="mode-option-title">Overlap (Polyphonic)</span>
                  <span className="mode-option-desc">Spawns multiple simultaneous overlapping instances</span>
                </div>
              </div>
            </div>

            {/* Per-Audio Volume */}
            <div className="form-group">
              <label className="form-label">Per-Sound Volume ({Math.round(formVolume * 100)}%)</label>
              <input
                type="range"
                min="0"
                max="1"
                step="0.05"
                value={formVolume}
                onChange={(e) => setFormVolume(parseFloat(e.target.value))}
                className="volume-slider"
                style={{ width: "100%" }}
              />
            </div>

            {/* Interactive Hotkey Recorder Box */}
            <div className="form-group">
              <label className="form-label">Hotkey Combination (Supports CTRL, ALT, SHIFT, etc.)</label>
              <div
                ref={recorderRef}
                tabIndex={0}
                className={`hotkey-recorder-box ${isRecordingHotkey ? "recording" : ""}`}
                onClick={() => {
                  setIsRecordingHotkey(true);
                  setRecordingPreview("Press CTRL, ALT, SHIFT + Key...");
                }}
                onKeyDown={isRecordingHotkey ? handleRecordKeyDown : undefined}
                title="Click to record keyboard hotkey combination"
              >
                <div className="hotkey-recorder-content">
                  <Keyboard size={18} style={{ color: "var(--color-primary)" }} />
                  {isRecordingHotkey ? (
                    <span style={{ color: "var(--color-primary)" }}>
                      {recordingPreview || "Press any key..."}
                    </span>
                  ) : formHotkey ? (
                    <span className="hotkey-key-pill">{formHotkey}</span>
                  ) : (
                    <span style={{ color: "var(--text-secondary)", fontSize: "12px" }}>
                      Click here to record (e.g. CTRL+1, CTRL+ALT+A)
                    </span>
                  )}
                </div>

                {formHotkey && !isRecordingHotkey && (
                  <button
                    type="button"
                    className="clear-btn"
                    onClick={(e) => {
                      e.stopPropagation();
                      setFormHotkey("");
                    }}
                    title="Clear Hotkey"
                  >
                    <X size={15} />
                  </button>
                )}
              </div>
            </div>

            <div className="modal-footer">
              <button className="btn-secondary" onClick={() => setIsModalOpen(false)}>
                Cancel
              </button>
              <button
                className="btn-primary"
                onClick={handleSaveSound}
                disabled={!formPath || !formTitle}
              >
                Save
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Modal Dual Audio Output Routing */}
      {isAudioSettingsOpen && (
        <div className="modal-overlay" onClick={() => setIsAudioSettingsOpen(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="modal-title">Dual Output Audio Routing</h3>
            </div>

            <p style={{ fontSize: "12px", color: "var(--text-secondary)", marginBottom: "16px", lineHeight: "1.4" }}>
              Route audio simultaneously to two separate outputs: one for personal monitoring (Speakers/Headphones) and one for broadcasting (Virtual Audio Cable / Discord Mic).
            </p>

            <div className="form-group">
              <label className="form-label">
                <Headphones size={14} style={{ display: "inline", verticalAlign: "middle", marginRight: "6px" }} />
                Output 1: Monitoring (Headphones / Personal Speakers)
              </label>
              <select
                className="form-select"
                value={formPrimaryDevice}
                onChange={(e) => setFormPrimaryDevice(e.target.value)}
              >
                <option value="NONE">None (Disabled)</option>
                {audioDevices.map((d) => (
                  <option key={`p_${d.name}`} value={d.name}>
                    {d.name} {d.is_default ? "(OS Default)" : ""}
                  </option>
                ))}
              </select>
            </div>

            <div className="form-group">
              <label className="form-label">
                <Mic size={14} style={{ display: "inline", verticalAlign: "middle", marginRight: "6px" }} />
                Output 2: Broadcast (Virtual Audio Cable / Discord Mic)
              </label>
              <select
                className="form-select"
                value={formSecondaryDevice}
                onChange={(e) => setFormSecondaryDevice(e.target.value)}
              >
                <option value="NONE">None (Disabled)</option>
                {audioDevices.map((d) => (
                  <option key={`s_${d.name}`} value={d.name}>
                    {d.name}
                  </option>
                ))}
              </select>
            </div>

            <div className="modal-footer">
              <button className="btn-secondary" onClick={() => setIsAudioSettingsOpen(false)}>
                Close
              </button>
              <button className="btn-primary" onClick={handleSaveAudioDevices}>
                Apply Routing
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Modal Remote Wi-Fi Server Port & Security Settings */}
      {isNetworkModalOpen && (
        <div className="modal-overlay" onClick={() => setIsNetworkModalOpen(false)}>
          <div className="modal-content" onClick={(e) => e.stopPropagation()}>
            <div className="modal-header">
              <h3 className="modal-title">Remote Server & Security Settings (Wi-Fi)</h3>
            </div>

            <p style={{ fontSize: "12px", color: "var(--text-secondary)", marginBottom: "16px", lineHeight: "1.4" }}>
              Configure the WebSocket port and security password for companion Android remote clients on your local network (Wi-Fi).
            </p>

            <div className="form-group">
              <label className="form-label">Host IP Address (Local)</label>
              <input
                type="text"
                className="form-input"
                value={networkInfo.ip}
                disabled
                style={{ opacity: 0.75, cursor: "not-allowed" }}
              />
            </div>

            <div className="form-group">
              <label className="form-label">WebSocket Port (1024 - 65535)</label>
              <input
                type="number"
                className="form-input"
                placeholder="58900"
                min={1024}
                max={65535}
                value={formPort}
                onChange={(e) => {
                  setFormPort(e.target.value);
                  setPortError("");
                }}
              />
            </div>

            <div className="form-group">
              <label className="form-label">
                <Lock size={13} style={{ display: "inline", verticalAlign: "middle", marginRight: "6px" }} />
                Remote Password (Optional)
              </label>
              <div style={{ display: "flex", gap: "8px" }}>
                <input
                  type={showPassword ? "text" : "password"}
                  className="form-input"
                  placeholder="Leave empty for no password..."
                  value={remotePassword}
                  onChange={(e) => setRemotePassword(e.target.value)}
                />
                <button
                  className="btn-secondary"
                  type="button"
                  onClick={() => setShowPassword((prev) => !prev)}
                  title={showPassword ? "Hide Password" : "Show Password"}
                >
                  {showPassword ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
              <p style={{ fontSize: "11px", color: "var(--text-secondary)", marginTop: "4px" }}>
                When set, Android remote clients must provide this password to connect and control playback.
              </p>
            </div>

            {portError && (
              <p style={{ color: "#EF4444", fontSize: "12px", marginBottom: "12px" }}>
                {portError}
              </p>
            )}

            <div className="modal-footer" style={{ display: "flex", justifyContent: "space-between" }}>
              <button className="btn-secondary" onClick={handleResetDefaultPort} type="button">
                Reset to Default
              </button>
              <div style={{ display: "flex", gap: "8px" }}>
                <button className="btn-secondary" onClick={() => setIsNetworkModalOpen(false)}>
                  Cancel
                </button>
                <button className="btn-primary" onClick={handleSaveNetworkSettings}>
                  Save & Apply
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Watermark in bottom right corner */}
      <footer className="app-watermark">
        <span>Made by</span>
        <button
          type="button"
          className="watermark-link"
          onClick={() => openUrl("https://github.com/anggixz/OpenSoundBoard")}
          title="Visit GitHub repository by Anggixz"
        >
          Anggixz
        </button>
      </footer>
    </div>
  );
}
