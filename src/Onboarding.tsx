import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { applyTheme, type ThemeName } from "./theme";

type Channel = {
  id: string;
  kind: string;
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

const DEMO_BARS = [6, 10, 16, 12, 20, 14, 8, 18, 22, 11, 7, 15, 19, 9, 13, 17, 8, 12];
const STEP_COUNT = 4;

function ToggleRow({
  on,
  disabled,
  label,
  title,
  subtitle,
  onToggle,
}: {
  on: boolean;
  disabled: boolean;
  label: string;
  title: string;
  subtitle: string;
  onToggle: () => void;
}) {
  return (
    <div className="onboard-toggle-row">
      <button
        type="button"
        className={`onboard-toggle ${on ? "on" : ""}`}
        aria-pressed={on}
        aria-label={label}
        disabled={disabled}
        onClick={onToggle}
      >
        <span />
      </button>
      <div className="onboard-toggle-copy">
        <strong>{title}</strong>
        <em>{subtitle}</em>
      </div>
    </div>
  );
}

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
    });
}

function folderLabel(path: string) {
  const parts = path.split(/[/\\]/).filter(Boolean);
  return parts.slice(-2).join(" / ") || path || "Documents / Cue";
}

export default function Onboarding() {
  const [step, setStep] = useState(0);
  const [settings, setSettings] = useState<SettingsData | null>(null);

  useEffect(() => {
    invoke<SettingsData>("get_settings")
      .then((loaded) => {
        applyTheme(loaded.theme);
        setSettings(loaded);
      })
      .catch(() => {});

    let cancelled = false;
    let unlisten: (() => void) | undefined;
    listen("onboarding-shown", () => {
      setStep(0);
      invoke<SettingsData>("get_settings")
        .then((loaded) => {
          if (!cancelled) {
            applyTheme(loaded.theme);
            setSettings(loaded);
          }
        })
        .catch(() => {});
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const persist = useCallback(async (next: SettingsData) => {
    applyTheme(next.theme);
    try {
      const saved = await invoke<SettingsData>("update_settings", { settings: next });
      setSettings(saved);
    } catch {
      setSettings(next);
    }
  }, []);

  const finish = useCallback(() => {
    void invoke("complete_onboarding");
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.metaKey || event.ctrlKey || event.altKey) return;
      if (event.key === "Enter") {
        if (event.target instanceof HTMLElement && event.target.closest("button")) return;
        event.preventDefault();
        if (step < STEP_COUNT - 1) setStep((s) => s + 1);
        else finish();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        if (step > 0) setStep((s) => s - 1);
        else finish();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [step, finish]);

  const keys = prettyShortcut(settings?.shortcut ?? "ctrl+shift+r");
  const folder = folderLabel(settings?.outputDir || settings?.defaultOutputDir || "");

  return (
    <div className="onboard">
      <header className="onboard-top">
        <div className="onboard-dots" aria-hidden="true">
          {Array.from({ length: STEP_COUNT }, (_, i) => (
            <span key={i} className={`onboard-dot ${i === step ? "on" : ""}`} />
          ))}
        </div>
        {step < STEP_COUNT - 1 ? (
          <button type="button" className="onboard-skip" onClick={finish}>
            Skip
          </button>
        ) : (
          <span className="onboard-skip-spacer" />
        )}
      </header>

      <div className="onboard-stage" key={step}>
        {step === 0 && (
          <>
            <div className="onboard-brand">
              <span className="onboard-brand-dot" aria-hidden="true" />
              <h1>Cue</h1>
            </div>
            <p className="onboard-lede">A recorder that stays out of the way.</p>
            <div className="demo-pill" aria-hidden="true">
              <span className="demo-rec" />
              <div className="demo-wave">
                {DEMO_BARS.map((height, i) => (
                  <span key={i} style={{ height: `${height}px` }} />
                ))}
              </div>
              <span className="demo-time">00:12</span>
              <span className="demo-chip">Voice</span>
            </div>
            <p className="onboard-body">
              It lives in the tray. A shortcut starts and stops — no window to keep around.
            </p>
          </>
        )}

        {step === 1 && (
          <>
            <h1>Press to record</h1>
            <p className="onboard-lede">Same shortcut starts and stops.</p>
            <div className="onboard-keys" aria-label={keys.join(" + ")}>
              {keys.map((key, i) => (
                <span key={`${key}-${i}`} className="onboard-key-wrap">
                  {i > 0 && <span className="onboard-plus">+</span>}
                  <kbd className="onboard-key">{key}</kbd>
                </span>
              ))}
            </div>
            <p className="onboard-body">
              Or click the tray icon. Hover the pill and press Cancel to throw it away,
              click the pill to save.
            </p>
          </>
        )}

        {step === 2 && (
          <>
            <h1>Wake on Voice</h1>
            <p className="onboard-lede">Optional. Off unless you turn it on.</p>
            <p className="onboard-body">
              Say “cue start recording” or “cue stop recording” instead of the shortcut.
            </p>
            <ToggleRow
              on={Boolean(settings?.wakeOnVoice)}
              disabled={!settings}
              label="Wake on Voice"
              title="Listen for “cue”"
              subtitle="Hands-free start and stop"
              onToggle={() => {
                if (!settings) return;
                void persist({ ...settings, wakeOnVoice: !settings.wakeOnVoice });
              }}
            />
            <p className="onboard-hint warn">
              Downloads a ~50MB voice model on first use. Always-on listening uses more
              battery.
            </p>
          </>
        )}

        {step === 3 && (
          <>
            <h1>You’re set</h1>
            <p className="onboard-lede">Takes land in {folder}.</p>
            <p className="onboard-body">
              After a save, O opens the folder and P plays the file. Right-click the tray
              icon for Settings — mics, apps, and voice commands.
            </p>
            <ToggleRow
              on={Boolean(settings?.launchAtLogin)}
              disabled={!settings}
              label="Start with Windows"
              title="Start with Windows"
              subtitle="Launch Cue in the tray when you sign in"
              onToggle={() => {
                if (!settings) return;
                void persist({ ...settings, launchAtLogin: !settings.launchAtLogin });
              }}
            />
            <p className="onboard-hint">
              Windows may ask for microphone access the first time you record.
            </p>
          </>
        )}
      </div>

      <footer className="onboard-foot">
        {step > 0 ? (
          <button type="button" className="onboard-back" onClick={() => setStep((s) => s - 1)}>
            Back
          </button>
        ) : (
          <span />
        )}
        {step < STEP_COUNT - 1 ? (
          <button type="button" className="onboard-next" onClick={() => setStep((s) => s + 1)}>
            Continue
          </button>
        ) : (
          <button type="button" className="onboard-next" onClick={finish}>
            Start using Cue
          </button>
        )}
      </footer>
    </div>
  );
}
