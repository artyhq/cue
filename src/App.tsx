import { useState, useEffect, useRef, useCallback } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { Check, CircleAlert, X } from "lucide-react";
import { playSfx, preloadSfx, attachSfxUnlock, setSfxEnabled } from "./sfx";
import "./theme";

const BAR_COUNT = 22;
const DISMISS_MS = 4000;
const LEAVE_MS = 280;

type SavedFile = { filename: string; path: string };

function pillPath(width: number, height: number) {
  const inset = 1.5;
  const x = inset;
  const y = inset;
  const w = Math.max(height, width - inset * 2);
  const h = Math.max(8, height - inset * 2);
  const r = h / 2;
  return [
    `M ${x + w / 2} ${y}`,
    `H ${x + w - r}`,
    `A ${r} ${r} 0 0 1 ${x + w} ${y + r}`,
    `V ${y + h - r}`,
    `A ${r} ${r} 0 0 1 ${x + w - r} ${y + h}`,
    `H ${x + r}`,
    `A ${r} ${r} 0 0 1 ${x} ${y + h - r}`,
    `V ${y + r}`,
    `A ${r} ${r} 0 0 1 ${x + r} ${y}`,
    "Z",
  ].join(" ");
}

const Waveform = ({ isRecording }: { isRecording: boolean }) => {
  const [levels, setLevels] = useState<number[]>(() => Array(BAR_COUNT).fill(0));

  useEffect(() => {
    let cancelled = false;
    let unlisten: UnlistenFn | undefined;

    listen<number[]>("cue://levels", (event) => {
      const next = event.payload.slice(-BAR_COUNT);
      while (next.length < BAR_COUNT) next.unshift(0);
      setLevels(next);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return (
    <div className="waveform" aria-hidden="true">
      {levels.map((level, i) => {
        const boosted = Math.min(1, Math.sqrt(Math.max(0, level)) * 1.8);
        const height = isRecording ? Math.min(16, Math.max(3, 3 + boosted * 13)) : 3;
        return <div key={i} className="bar" style={{ height: `${height}px` }} />;
      })}
    </div>
  );
};

function App() {
  // TASK: update tray icon for listening state (e.g. tray-listening.png) when settings.wakeOnVoice is true
  const [time, setTime] = useState(0);
  const [isRecording, setIsRecording] = useState(false);
  const [arming, setArming] = useState(false);
  const [saved, setSaved] = useState<SavedFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [presetName, setPresetName] = useState("Voice");
  const [leaving, setLeaving] = useState(false);
  const [hovered, setHovered] = useState(false);
  const [pressedKey, setPressedKey] = useState<"o" | "p" | null>(null);
  const [enterGen, setEnterGen] = useState(0);
  const [progress, setProgress] = useState(1);
  const [pillSize, setPillSize] = useState({ w: 180, h: 44 });

  const pillRef = useRef<HTMLDivElement>(null);
  const savedRef = useRef<SavedFile | null>(null);
  const errorRef = useRef<string | null>(null);
  const remainingRef = useRef(DISMISS_MS);
  const leavingRef = useRef(false);
  const liveRef = useRef(false);
  const timerWarnedRef = useRef(false);
  const armGenRef = useRef(0);
  const leaveTimerRef = useRef<number>(0);
  savedRef.current = saved;
  errorRef.current = error;

  const hideOverlay = useCallback(async () => {
    const win = getCurrentWebviewWindow();
    await win.hide();
    await win.emit("overlay-hidden");
    setSaved(null);
    setError(null);
    setLeaving(false);
    setHovered(false);
    setProgress(1);
    leavingRef.current = false;
    liveRef.current = false;
    timerWarnedRef.current = false;
    armGenRef.current += 1;
    setArming(false);
    remainingRef.current = DISMISS_MS;
  }, []);

  const beginLeave = useCallback(
    (reason: "timeout" | "escape" | "action" | "cancel" = "timeout") => {
      if (leavingRef.current) return;
      leavingRef.current = true;
      if (reason === "cancel") playSfx("recordCancel");
      else if (reason !== "action") playSfx("dismiss");
      setLeaving(true);
      window.clearTimeout(leaveTimerRef.current);
      leaveTimerRef.current = window.setTimeout(() => {
        void hideOverlay();
      }, LEAVE_MS);
    },
    [hideOverlay],
  );

  const dismiss = useCallback(
    (reason: "timeout" | "escape" | "action" = "timeout") => {
      beginLeave(reason);
    },
    [beginLeave],
  );

  const flashKey = (key: "o" | "p") => {
    setPressedKey(key);
    window.setTimeout(() => {
      setPressedKey((current) => (current === key ? null : current));
    }, 150);
  };

  const openFolder = useCallback(async () => {
    const current = savedRef.current;
    if (!current) return;
    flashKey("o");
    playSfx("openFolder");
    try {
      await invoke("reveal_saved", { path: current.path });
    } catch {
      await invoke("open_output_dir");
    }
    dismiss("action");
  }, [dismiss]);

  const playFile = useCallback(async () => {
    const current = savedRef.current;
    if (!current) return;
    flashKey("p");
    playSfx("playFile");
    try {
      await invoke("open_saved", { path: current.path });
    } catch {
      // player may still open; don't leave the pill stuck
    }
    dismiss("action");
  }, [dismiss]);

  const cancelRecording = useCallback(
    (event?: { stopPropagation(): void }) => {
      event?.stopPropagation();
      beginLeave("cancel");
      void invoke("cancel_recording_cmd");
    },
    [beginLeave],
  );

  useEffect(() => {
    if (!saved && !error) return;
    const node = pillRef.current;
    if (!node) return;
    const sync = () => {
      const rect = node.getBoundingClientRect();
      setPillSize({ w: rect.width, h: rect.height });
    };
    sync();
    const observer = new ResizeObserver(sync);
    observer.observe(node);
    node.focus({ preventScroll: true });
    return () => observer.disconnect();
  }, [saved, error]);

  useEffect(() => {
    if (!isRecording) return;
    const interval = setInterval(() => {
      setTime((t) => t + 1);
    }, 1000);
    return () => clearInterval(interval);
  }, [isRecording]);

  useEffect(() => {
    if ((!saved && !error) || hovered || leaving) return;
    let frame = 0;
    let last = performance.now();
    const tick = (now: number) => {
      remainingRef.current -= now - last;
      last = now;
      setProgress(Math.max(0, remainingRef.current / DISMISS_MS));
      if (
        saved &&
        !timerWarnedRef.current &&
        remainingRef.current <= 800 &&
        remainingRef.current > 0
      ) {
        timerWarnedRef.current = true;
        playSfx("timerWarn");
      }
      if (remainingRef.current <= 0) {
        dismiss("timeout");
        return;
      }
      frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [saved, error, hovered, leaving, dismiss]);

  useEffect(() => {
    preloadSfx();
    const detachUnlock = attachSfxUnlock();
    let cancelled = false;
    const unlistens: UnlistenFn[] = [];

    const track = (fn: UnlistenFn) => {
      if (cancelled) fn();
      else unlistens.push(fn);
    };

    const beginRecording = () => {
      liveRef.current = true;
      leavingRef.current = false;
      remainingRef.current = DISMISS_MS;
      timerWarnedRef.current = false;
      setArming(false);
      setIsRecording(true);
      setSaved(null);
      setError(null);
      setTime(0);
      setLeaving(false);
      setHovered(false);
      setProgress(1);
    };

    const setup = async () => {
      const appWindow = getCurrentWebviewWindow();

      track(
        await appWindow.listen("overlay-shown", () => {
          const gen = ++armGenRef.current;
          liveRef.current = false;
          leavingRef.current = false;
          window.clearTimeout(leaveTimerRef.current);
          setArming(true);
          setIsRecording(false);
          setSaved(null);
          setError(null);
          setTime(0);
          setLeaving(false);
          setHovered(false);
          setProgress(1);
          setEnterGen((n) => n + 1);
          void (async () => {
            await playSfx("recordStart");
            if (gen !== armGenRef.current) return;
            try {
              await invoke("start_recording_cmd");
            } catch {
              // cue://error updates the pill
            }
          })();
        }),
      );
      track(
        await listen<string>("cue://recording-started", (event) => {
          if (event.payload) setPresetName(event.payload);
          beginRecording();
        }),
      );

      invoke<{ activePresetId: string; presets: { id: string; name: string }[] }>("get_settings")
        .then((loaded) => {
          const name = loaded.presets.find((p) => p.id === loaded.activePresetId)?.name;
          if (name) setPresetName(name);
        })
        .catch(() => {});

      track(
        await listen<{ activePresetId: string; presets: { id: string; name: string }[] }>(
          "cue://settings-changed",
          (event) => {
            const name = event.payload.presets.find(
              (p) => p.id === event.payload.activePresetId,
            )?.name;
            if (name) setPresetName(name);
          },
        ),
      );

      track(
        await appWindow.listen("overlay-hidden", () => {
          armGenRef.current += 1;
          setArming(false);
          setIsRecording(false);
        }),
      );

      track(
        await listen<SavedFile>("cue://stopped", (event) => {
          liveRef.current = false;
          timerWarnedRef.current = false;
          setIsRecording(false);
          setArming(false);
          remainingRef.current = DISMISS_MS;
          leavingRef.current = false;
          setProgress(1);
          setLeaving(false);
          setHovered(false);
          setSaved(event.payload);
          playSfx("recordStop");
          void getCurrentWebviewWindow().setFocus();
          requestAnimationFrame(() => pillRef.current?.focus({ preventScroll: true }));
        }),
      );

      track(
        await listen("cue://cancelled", () => {
          liveRef.current = false;
          armGenRef.current += 1;
          setArming(false);
          setIsRecording(false);
          setSaved(null);
          if (!leavingRef.current) beginLeave("cancel");
        }),
      );

      track(
        await listen<string>("cue://error", (event) => {
          liveRef.current = false;
          armGenRef.current += 1;
          timerWarnedRef.current = false;
          remainingRef.current = DISMISS_MS;
          leavingRef.current = false;
          playSfx("error");
          setArming(false);
          setIsRecording(false);
          setSaved(null);
          setProgress(1);
          setLeaving(false);
          setError(event.payload);
          void getCurrentWebviewWindow().setFocus();
          requestAnimationFrame(() => pillRef.current?.focus({ preventScroll: true }));
        }),
      );

      track(
        await listen<boolean>("cue://sfx-enabled", (event) => {
          setSfxEnabled(event.payload);
        }),
      );
    };

    setup();

    return () => {
      cancelled = true;
      detachUnlock();
      window.clearTimeout(leaveTimerRef.current);
      unlistens.forEach((fn) => fn());
    };
  }, [beginLeave]);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      if (errorRef.current) {
        if (e.key === "Escape") {
          e.preventDefault();
          dismiss("escape");
        }
        return;
      }
      if (savedRef.current) {
        if (e.key === "Escape") {
          e.preventDefault();
          dismiss("escape");
          return;
        }
        if (e.key === "o" || e.key === "O") {
          e.preventDefault();
          void openFolder();
          return;
        }
        if (e.key === "p" || e.key === "P") {
          e.preventDefault();
          void playFile();
        }
        return;
      }
      if (e.key === "Escape") {
        cancelRecording();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [dismiss, openFolder, playFile, cancelRecording]);

  const formatTime = (seconds: number) => {
    const m = Math.floor(seconds / 60)
      .toString()
      .padStart(2, "0");
    const s = (seconds % 60).toString().padStart(2, "0");
    return `${m}:${s}`;
  };

  const handlePillClick = async () => {
    if (error) {
      dismiss("escape");
      return;
    }
    if (saved) return;
    if (arming) {
      cancelRecording();
      return;
    }
    if (isRecording) {
      await invoke("stop_recording_cmd");
    }
  };

  const showingSaved = Boolean(saved) && !error;
  const showingNotice = showingSaved || Boolean(error);
  const isLive = (arming || isRecording) && !showingNotice;
  const d = Math.max(0, progress) * 100;

  return (
    <div
      key={enterGen}
      ref={pillRef}
      tabIndex={-1}
      className={[
        "pill-container",
        isLive ? "is-live" : "",
        arming ? "is-arming" : "",
        isRecording ? "is-recording" : "",
        showingNotice ? "is-saved" : "",
        leaving ? "is-leaving" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      onClick={handlePillClick}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {showingSaved && (
        <svg
          className="pill-timer"
          width={pillSize.w}
          height={pillSize.h}
          viewBox={`0 0 ${pillSize.w} ${pillSize.h}`}
          aria-hidden="true"
        >
          <path className="pill-timer-track" d={pillPath(pillSize.w, pillSize.h)} />
          <path
            className="pill-timer-fuse"
            d={pillPath(pillSize.w, pillSize.h)}
            pathLength={100}
            strokeDasharray={`${d} 100`}
          />
        </svg>
      )}
      {error ? (
        <div className="saved-state">
          <span className="error-mark" aria-hidden="true">
            <CircleAlert size={14} strokeWidth={2.25} />
          </span>
          <span className="saved-text" title={error}>
            {error}
          </span>
        </div>
      ) : saved ? (
        <div className="saved-state">
          <span className="saved-mark" aria-hidden="true">
            <Check size={14} strokeWidth={2.6} />
          </span>
          <span className="saved-text" title={saved.filename}>
            Saved
          </span>
          <div className="saved-actions">
            <button
              type="button"
              className={`keycap ${pressedKey === "o" ? "is-pressed" : ""}`}
              title="Open folder"
              aria-label="Open folder"
              onClick={(e) => {
                e.stopPropagation();
                void openFolder();
              }}
            >
              O
            </button>
            <button
              type="button"
              className={`keycap ${pressedKey === "p" ? "is-pressed" : ""}`}
              title="Play recording"
              aria-label="Play recording"
              onClick={(e) => {
                e.stopPropagation();
                void playFile();
              }}
            >
              P
            </button>
          </div>
        </div>
      ) : (
        <>
          <div className="record-dot" />
          <Waveform key={isRecording ? "on" : "off"} isRecording={isRecording} />
          <div className="time">{formatTime(time)}</div>
          <div className="chip">{presetName}</div>
          <div className="pill-reveal" aria-hidden={!hovered}>
            <div className="pill-reveal-clip">
              <div className="pill-reveal-cluster">
                <span className="pill-reveal-rule" />
                <button
                  type="button"
                  className="pill-icon-btn is-cancel"
                  title="Cancel"
                  aria-label="Cancel recording"
                  onClick={cancelRecording}
                >
                  <X size={14} strokeWidth={2.4} />
                </button>
              </div>
            </div>
          </div>
        </>
      )}
    </div>
  );
}

export default App;
