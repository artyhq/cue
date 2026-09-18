import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";
import { applyTheme, type ThemeName } from "./theme";
import { isSfxEnabled, setSfxEnabled } from "./sfx";

type AudioDevice = {
  id: string;
  name: string;
  isDefault: boolean;
};

type Channel = {
  id: string;
  kind: "mic" | "system" | "app" | string;
  enabled: boolean;
  name: string;
  exe: string;
};

type Preset = {
  id: string;
  name: string;
  channels: Channel[];
};

type SettingsData = {
  theme: ThemeName;
  inputDeviceId: string;
  outputDeviceId: string;
  outputDir: string;
  defaultOutputDir: string;
  activePresetId: string;
  presets: Preset[];
  shortcut: string;
  indicator: string;
  launchAtLogin: boolean;
  wakeOnVoice: boolean;
  onboarded: boolean;
  onboardingVersion: number;
};

type PlayingApp = {
  exe: string;
  name: string;
};

const THEMES: { id: ThemeName; label: string }[] = [
  { id: "system", label: "System" },
  { id: "light", label: "Light" },
  { id: "dark", label: "Dark" },
];

const SYSTEM_DEVICE = { id: "", name: "System default", isDefault: false };

const INDICATORS: { id: string; label: string }[] = [
  { id: "pill", label: "Pill" },
  { id: "tray", label: "Tray" },
];

function prettyShortcut(value: string) {
  return value
    .split("+")
    .filter(Boolean)
    .map((part) => {
      const key = part.toLowerCase();
      if (key === "ctrl" || key === "control") return "Ctrl";
      if (key === "shift") return "Shift";
      if (key === "alt" || key === "option") return "Alt";
      if (key === "super" || key === "cmd" || key === "command" || key === "win") return "Win";
      if (key.length === 1) return key.toUpperCase();
      return part;
    })
    .join(" + ");
}

function eventToShortcut(event: KeyboardEvent): string | null {
  if (event.repeat) return null;
  const mods: string[] = [];
  if (event.ctrlKey) mods.push("ctrl");
  if (event.metaKey) mods.push("super");
  if (event.altKey) mods.push("alt");
  if (event.shiftKey) mods.push("shift");
  if (mods.length === 0) return null;

  const code = event.code;
  let key: string | null = null;
  if (code.startsWith("Key") && code.length === 4) key = code.slice(3).toLowerCase();
  else if (code.startsWith("Digit") && code.length === 6) key = code.slice(5);
  else if (/^F\d{1,2}$/.test(code)) key = code.toLowerCase();
  else {
    const extras: Record<string, string> = {
      Space: "space",
      Minus: "-",
      Equal: "=",
      BracketLeft: "[",
      BracketRight: "]",
      Semicolon: ";",
      Quote: "'",
      Comma: ",",
      Period: ".",
      Slash: "/",
      Backslash: "\\",
      Backquote: "`",
    };
    key = extras[code] ?? null;
  }
  if (!key) return null;
  return [...mods, key].join("+");
}

function deviceLabel(device: AudioDevice) {
  return device.isDefault ? `${device.name} · Default` : device.name;
}

function withSelected(list: AudioDevice[], id: string) {
  if (!id || list.some((device) => device.id === id)) return list;
  return [...list, { id, name: "Unavailable device", isDefault: false }];
}

function activePreset(settings: SettingsData): Preset {
  return (
    settings.presets.find((p) => p.id === settings.activePresetId) ??
    settings.presets[0]
  );
}

export default function Settings() {
  const [settings, setSettings] = useState<SettingsData | null>(null);
  const [inputs, setInputs] = useState<AudioDevice[]>([SYSTEM_DEVICE]);
  const [outputs, setOutputs] = useState<AudioDevice[]>([SYSTEM_DEVICE]);
  const [peak, setPeak] = useState(0);
  const [apps, setApps] = useState<PlayingApp[]>([]);
  const [pickingApp, setPickingApp] = useState(false);
  const [editingName, setEditingName] = useState(false);
  const [soundOn, setSoundOn] = useState(true);
  const [listening, setListening] = useState(false);
  const [shortcutError, setShortcutError] = useState<string | null>(null);

  const persist = useCallback(async (next: SettingsData) => {
    applyTheme(next.theme);
    try {
      const saved = await invoke<SettingsData>("update_settings", { settings: next });
      setShortcutError(null);
      setSettings(saved);
      return saved;
    } catch (error) {
      setShortcutError(String(error));
      throw error;
    }
  }, []);

  const refreshMonitor = useCallback(async () => {
    await invoke("start_input_monitor");
  }, []);

  useEffect(() => {
    let cancelled = false;
    const unlistens: UnlistenFn[] = [];

    const boot = async () => {
      const [loaded, inputList, outputList] = await Promise.all([
        invoke<SettingsData>("get_settings"),
        invoke<AudioDevice[]>("list_audio_devices", { kind: "input" }),
        invoke<AudioDevice[]>("list_audio_devices", { kind: "output" }),
      ]);
      if (cancelled) return;
      applyTheme(loaded.theme);
      setSoundOn(isSfxEnabled());
      setSettings(loaded);
      setInputs(withSelected([SYSTEM_DEVICE, ...inputList], loaded.inputDeviceId));
      setOutputs(withSelected([SYSTEM_DEVICE, ...outputList], loaded.outputDeviceId));
      await invoke("start_input_monitor");
    };

    boot();

    listen<number>("cue://monitor-level", (event) => {
      setPeak(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlistens.push(fn);
    });

    listen<number[]>("cue://levels", (event) => {
      setPeak(Math.max(0, ...event.payload));
    }).then((fn) => {
      if (cancelled) fn();
      else unlistens.push(fn);
    });

    listen<SettingsData>("cue://settings-changed", (event) => {
      setSettings(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlistens.push(fn);
    });

    return () => {
      cancelled = true;
      unlistens.forEach((fn) => fn());
      invoke("stop_input_monitor");
    };
  }, []);

  useEffect(() => {
    if (!listening) return;
    const onKey = (event: KeyboardEvent) => {
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Escape") {
        setListening(false);
        return;
      }
      const next = eventToShortcut(event);
      if (!next || !settings) return;
      setListening(false);
      void persist({ ...settings, shortcut: next }).catch(() => {});
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [listening, persist, settings]);

  if (!settings) {
    return <div className="settings-shell" />;
  }

  const preset = activePreset(settings);
  const folder = settings.outputDir || settings.defaultOutputDir;
  const customFolder = settings.outputDir.length > 0;

  const patch = (partial: Partial<SettingsData>) => {
    void persist({ ...settings, ...partial }).then(() => refreshMonitor());
  };

  const updatePreset = (nextPreset: Preset) => {
    const presets = settings.presets.map((p) => (p.id === nextPreset.id ? nextPreset : p));
    patch({ presets });
  };

  const selectPreset = (id: string) => {
    patch({ activePresetId: id });
  };

  const addPreset = () => {
    const n = settings.presets.length + 1;
    const presetNew: Preset = {
      id: `preset-${Date.now()}`,
      name: `Setup ${n}`,
      channels: [
        { id: "mic", kind: "mic", enabled: true, name: "My voice", exe: "" },
        { id: "system", kind: "system", enabled: true, name: "Computer sound", exe: "" },
      ],
    };
    patch({ presets: [...settings.presets, presetNew], activePresetId: presetNew.id });
    setEditingName(true);
  };

  const removePreset = (id: string) => {
    if (settings.presets.length <= 1) return;
    const presets = settings.presets.filter((p) => p.id !== id);
    const activePresetId =
      settings.activePresetId === id ? presets[0].id : settings.activePresetId;
    patch({ presets, activePresetId });
  };

  const toggleChannel = (id: string) => {
    updatePreset({
      ...preset,
      channels: preset.channels.map((ch) =>
        ch.id === id ? { ...ch, enabled: !ch.enabled } : ch,
      ),
    });
  };

  const removeChannel = (id: string) => {
    updatePreset({
      ...preset,
      channels: preset.channels.filter((ch) => ch.id !== id),
    });
  };

  const openAppPicker = async () => {
    setPickingApp(true);
    try {
      const list = await invoke<PlayingApp[]>("list_playing_apps");
      setApps(list);
    } catch {
      setApps([]);
    }
  };

  const addApp = (app: PlayingApp) => {
    const id = `app:${app.exe.toLowerCase()}`;
    if (preset.channels.some((ch) => ch.id === id)) {
      setPickingApp(false);
      return;
    }
    updatePreset({
      ...preset,
      channels: [
        ...preset.channels,
        { id, kind: "app", enabled: true, name: app.name, exe: app.exe },
      ],
    });
    setPickingApp(false);
  };

  const onPickFolder = async () => {
    const picked = await invoke<string | null>("pick_output_dir");
    if (picked) patch({ outputDir: picked });
  };

  return (
    <div className="settings-shell">
      <div className="settings">
        <section>
          <h2>What to record</h2>
          <div className="presets">
            {settings.presets.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`preset-chip ${item.id === preset.id ? "on" : ""}`}
                onClick={() => selectPreset(item.id)}
              >
                {item.name}
              </button>
            ))}
            <button type="button" className="preset-chip add" onClick={addPreset}>
              +
            </button>
          </div>

          {editingName ? (
            <input
              className="name-input"
              value={preset.name}
              autoFocus
              onChange={(e) => updatePreset({ ...preset, name: e.target.value })}
              onBlur={() => setEditingName(false)}
              onKeyDown={(e) => {
                if (e.key === "Enter") setEditingName(false);
              }}
            />
          ) : (
            <button type="button" className="name-btn" onClick={() => setEditingName(true)}>
              {preset.name}
              <span>Rename</span>
            </button>
          )}

          <div className="channels">
            {preset.channels.map((ch) => (
              <div key={ch.id} className={`channel ${ch.enabled ? "on" : ""}`}>
                <button
                  type="button"
                  className={`toggle ${ch.enabled ? "on" : ""}`}
                  aria-pressed={ch.enabled}
                  onClick={() => toggleChannel(ch.id)}
                >
                  <span />
                </button>
                <div className="channel-copy">
                  <strong>{ch.name}</strong>
                  {ch.kind === "app" && <em>{ch.exe}</em>}
                  {ch.kind === "mic" && <em>Microphone</em>}
                  {ch.kind === "system" && <em>Everything your computer plays</em>}
                </div>
                {ch.kind === "app" && (
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label={`Remove ${ch.name}`}
                    onClick={() => removeChannel(ch.id)}
                  >
                    ×
                  </button>
                )}
              </div>
            ))}
          </div>

          {pickingApp ? (
            <div className="app-picker">
              <div className="picker-head">
                <span>Apps playing sound</span>
                <button type="button" className="text-btn" onClick={openAppPicker}>
                  Refresh
                </button>
              </div>
              {apps.length === 0 ? (
                <p className="hint">
                  Play something in Spotify, a browser, or a call, then tap Refresh.
                </p>
              ) : (
                apps.map((app) => (
                  <button
                    key={app.exe}
                    type="button"
                    className="app-row"
                    onClick={() => addApp(app)}
                  >
                    <strong>{app.name}</strong>
                    <span>Add</span>
                  </button>
                ))
              )}
              <button type="button" className="text-btn" onClick={() => setPickingApp(false)}>
                Cancel
              </button>
            </div>
          ) : (
            <button type="button" className="add-app" onClick={openAppPicker}>
              + Add an app
            </button>
          )}

          <div className="meter" aria-hidden="true">
            <div className="meter-fill" style={{ transform: `scaleX(${Math.min(1, peak * 2.4)})` }} />
          </div>
        </section>

        <section>
          <h2>Devices</h2>
          <label className="field">
            <span>Microphone</span>
            <div className="select-wrap">
              <select
                value={settings.inputDeviceId}
                onChange={(e) => patch({ inputDeviceId: e.target.value })}
              >
                {inputs.map((device) => (
                  <option key={device.id || "system"} value={device.id}>
                    {deviceLabel(device)}
                  </option>
                ))}
              </select>
            </div>
          </label>
          <label className="field">
            <span>Computer sound from</span>
            <div className="select-wrap">
              <select
                value={settings.outputDeviceId}
                onChange={(e) => patch({ outputDeviceId: e.target.value })}
              >
                {outputs.map((device) => (
                  <option key={device.id || "system"} value={device.id}>
                    {deviceLabel(device)}
                  </option>
                ))}
              </select>
            </div>
          </label>
        </section>

        <section>
          <h2>Sound</h2>
          <div className="channel">
            <button
              type="button"
              className={`toggle ${soundOn ? "on" : ""}`}
              aria-pressed={soundOn}
              aria-label="Pill sounds"
              onClick={() => {
                const next = !soundOn;
                setSoundOn(next);
                setSfxEnabled(next);
                void emit("cue://sfx-enabled", next);
              }}
            >
              <span />
            </button>
            <div className="channel-copy">
              <strong>Recording sounds</strong>
              <em>Confirms start, save, and keys by ear</em>
            </div>
          </div>
        </section>

        <section>
          <h2>Indicator</h2>
          <div className="segmented two" role="radiogroup" aria-label="Record indicator">
            {INDICATORS.map((item) => (
              <button
                key={item.id}
                type="button"
                role="radio"
                aria-checked={settings.indicator === item.id}
                className={settings.indicator === item.id ? "on" : ""}
                onClick={() => patch({ indicator: item.id })}
              >
                {item.label}
              </button>
            ))}
          </div>
          <p className="hint">
            {settings.indicator === "tray"
              ? "Only the tray icon changes while you record"
              : "The pill and tray both show that you’re recording"}
          </p>
        </section>

        <section>
          <h2>Shortcut</h2>
          <button
            type="button"
            className={`shortcut-btn ${listening ? "listening" : ""}`}
            onClick={() => {
              setShortcutError(null);
              setListening(true);
            }}
          >
            {listening ? "Press a shortcut" : prettyShortcut(settings.shortcut)}
          </button>
          {shortcutError ? (
            <p className="hint danger-text">{shortcutError}</p>
          ) : (
            <p className="hint">Include Ctrl, Alt, or Win. Esc cancels.</p>
          )}
        </section>

        <section>
          <h2>System</h2>
          <div className="channel">
            <button
              type="button"
              className={`toggle ${settings.launchAtLogin ? "on" : ""}`}
              aria-pressed={settings.launchAtLogin}
              aria-label="Start with Windows"
              onClick={() => patch({ launchAtLogin: !settings.launchAtLogin })}
            >
              <span />
            </button>
            <div className="channel-copy">
              <strong>Start with Windows</strong>
              <em>Launch Cue in the tray when you sign in</em>
            </div>
          </div>
        </section>

        <section>
          <h2>Voice Commands</h2>
          <div className="channel">
            <button
              type="button"
              className={`toggle ${settings.wakeOnVoice ? "on" : ""}`}
              aria-pressed={settings.wakeOnVoice}
              aria-label="Wake on Voice"
              onClick={() => patch({ wakeOnVoice: !settings.wakeOnVoice })}
            >
              <span />
            </button>
            <div className="channel-copy">
              <strong>Wake on Voice</strong>
              <em>Say "cue start recording" or "cue stop recording". Downloads a ~50MB voice model on first use.</em>
            </div>
          </div>
          <p className="hint warning-text" style={{ marginTop: '8px' }}>
            Warning: Always-on listening will use more battery.
          </p>
        </section>

        <section>
          <h2>Appearance</h2>
          <div className="segmented" role="radiogroup" aria-label="Theme">
            {THEMES.map((theme) => (
              <button
                key={theme.id}
                type="button"
                role="radio"
                aria-checked={settings.theme === theme.id}
                className={settings.theme === theme.id ? "on" : ""}
                onClick={() => patch({ theme: theme.id })}
              >
                {theme.label}
              </button>
            ))}
          </div>
        </section>

        <section>
          <h2>Saved files</h2>
          <div className="field">
            <span>Folder</span>
            <div className="path-row">
              <div className="path" title={folder}>
                {folder}
              </div>
              <button type="button" className="chip-btn" onClick={onPickFolder}>
                Change
              </button>
            </div>
          </div>
          <div className="actions">
            <button type="button" className="text-btn" onClick={() => invoke("open_output_dir")}>
              Open folder
            </button>
            {customFolder && (
              <button type="button" className="text-btn" onClick={() => patch({ outputDir: "" })}>
                Use default
              </button>
            )}
            {settings.presets.length > 1 && (
              <button
                type="button"
                className="text-btn danger"
                onClick={() => removePreset(preset.id)}
              >
                Delete {preset.name}
              </button>
            )}
          </div>
        </section>

        <p className="footer">{prettyShortcut(settings.shortcut)} records this setup</p>
      </div>
    </div>
  );
}
