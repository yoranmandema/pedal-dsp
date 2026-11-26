import { useState, useEffect, useRef } from "preact/hooks";
import { state, closeGainStaging, bridge, setInputGain } from "../state";
import type { GainStagingLevelEvent } from "../protocol";
import "./gain-staging.css";

// Instrument type definitions from the design spec
const INSTRUMENT_TYPES = [
  { id: "electric_guitar", name: "Electric Guitar", icon: "🎸", target: -10 },
  { id: "bass", name: "Bass", icon: "🎸", target: -12 },
  { id: "acoustic_guitar", name: "Acoustic Guitar", icon: "🪕", target: -8 },
  { id: "microphone", name: "Microphone", icon: "🎙️", target: -20 },
  { id: "line_in", name: "Line In", icon: "🔌", target: -2 },
  { id: "active_pickup", name: "Active Pickup", icon: "🎸", target: -6 },
  { id: "passive_pickup", name: "Passive Pickup", icon: "🎸", target: -14 },
  { id: "other", name: "Other", icon: "⚙️", target: -10 },
] as const;

/** Instrument type id (typeof INSTRUMENT_TYPES[number]["id"]). */
export type InstrumentType = typeof INSTRUMENT_TYPES[number]["id"];

interface GainStagingProfile {
  id: string;
  name: string;
  instrument: InstrumentType;
  gain_db: number;
}

type ModalStep = 1 | 2 | 3;
type MeasurePhase = "idle" | "countdown" | "playing" | "done";

export function GainStagingModal() {
  const s = state.value;
  if (!s || !s.gainStagingOpen) return null;

  return (
    <div class="gs-modal-backdrop" onClick={(e) => {
      if (e.target === e.currentTarget) closeGainStaging();
    }} onKeyDown={(e) => {
      if (e.key === "Escape") closeGainStaging();
    }} tabIndex={0} role="dialog" aria-modal="true">
      <div class="gs-modal-card">
        <div class="gs-modal-header">
          <span class="gs-modal-title">Gain Staging</span>
          <button class="gs-modal-close" onClick={() => closeGainStaging()} aria-label="Close">✕</button>
        </div>
        <GainStagingStep1 />
      </div>
    </div>
  );
}

function GainStagingStep1() {
  const [step, setStep] = useState<ModalStep>(1);
  const [selected, setSelected] = useState<InstrumentType>("electric_guitar");
  const [profileName, setProfileName] = useState("");
  const [profiles, setProfiles] = useState<GainStagingProfile[]>([]);
  const [showProfileInput, setShowProfileInput] = useState(false);
  const [measurePhase, setMeasurePhase] = useState<MeasurePhase>("idle");
  const [currentDb, setCurrentDb] = useState(0);
  const [peakDb, setPeakDb] = useState(-60);
  const [progress, setProgress] = useState(0);
  const [measuredAvg, setMeasuredAvg] = useState<number | null>(null);
  const [measuredPeak, setMeasuredPeak] = useState<number | null>(null);
  const [recommendedGain, setRecommendedGain] = useState<number | null>(null);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Load profiles and listen for gain staging events on mount
  useEffect(() => {
    sendGainStaging({ action: "list_profiles" });
    const onMessage = (e: MessageEvent) => {
      try {
        const msg = JSON.parse(String(e.data));
        if (msg.type === "event") {
          if (msg.event === "gain_staging_level") {
            setCurrentDb(msg.payload.level ?? -60);
            setPeakDb((prev) => Math.max(prev, msg.payload.peak_level ?? -60));
            setProgress((p) => Math.min(100, p + (100 / 5)));
          } else if (msg.event === "measure_complete") {
            setMeasuredAvg(msg.payload.recommended_input ?? null);
            setMeasuredPeak(msg.payload.recommended_output ?? null);
            setRecommendedGain(msg.payload.recommended_input ?? null);
            setMeasurePhase("done");
            setStep(3);
          } else if (msg.event === "gain_staging_profiles") {
            // Profile names arrived — update the modal's local list.
            const ev = msg as GainStagingLevelEvent;
            if (ev.event === "gain_staging_profiles") {
              const names = ev.payload.profiles ?? [];
              setProfiles(names.map((name) => ({ name }) as GainStagingProfile));
            }
          }
        }
      } catch { /* ignore */ }
    };
    const w = bridge.socket;
    if (w) w.addEventListener("message", onMessage);
    return () => {
      if (w) w.removeEventListener("message", onMessage);
    };
  }, []);

  const handleCancel = () => {
    if (countdownRef.current) clearInterval(countdownRef.current);
    setMeasurePhase("idle");
    setProgress(0);
    setPeakDb(-60);
    setCurrentDb(0);
    closeGainStaging();
  };

  const handleStartMeasure = () => {
    setMeasurePhase("countdown");
    setProgress(0);
    setPeakDb(-60);
    setCurrentDb(0);
    setMeasuredAvg(null);
    setMeasuredPeak(null);
    setRecommendedGain(null);

    // 5 second countdown
    let remaining = 5;
    const countdownInterval = setInterval(() => {
      remaining--;
      if (remaining <= 0) {
        clearInterval(countdownInterval);
        setMeasurePhase("playing");
        sendGainStaging({ action: "start_measure", channel: 0 });
      }
    }, 1000);
    countdownRef.current = countdownInterval as unknown as ReturnType<typeof setInterval>;
    setStep(2);
  };

  const handleSaveProfile = async () => {
    if (!profileName.trim()) return;
    sendGainStaging({ action: "save_profile", name: profileName.trim() });
    setShowProfileInput(false);
    setProfileName("");
  };

  const handleApply = () => {
    if (recommendedGain !== null) {
      sendGainStaging({ action: "apply", profile_name: "", input_trim: recommendedGain, output_trim: 0 });
      setInputGain(recommendedGain);
      closeGainStaging();
    }
  };

  const handleStartOver = () => {
    if (countdownRef.current) clearInterval(countdownRef.current);
    setStep(1);
    setMeasurePhase("idle");
    setProgress(0);
    setPeakDb(-60);
    setCurrentDb(0);
    setMeasuredAvg(null);
    setMeasuredPeak(null);
    setRecommendedGain(null);
  };

  const instrument = INSTRUMENT_TYPES.find((i) => i.id === selected);

  return (
    <div class="gs-step">
      {/* Step 1: Instrument Selection */}
      {step === 1 && (
        <div class="gs-instruments">
          <h3 class="gs-step-title">Select your instrument type</h3>
          <div class="gs-instrument-grid">
            {INSTRUMENT_TYPES.map((inst) => (
              <button
                key={inst.id}
                class={`gs-instrument-btn ${selected === inst.id ? "on" : ""}`}
                onClick={() => setSelected(inst.id)}
              >
                <span class="gs-inst-icon">{inst.icon}</span>
                <span class="gs-inst-name">{inst.name}</span>
                <span class="gs-inst-target">{inst.target >= 0 ? "+" : ""}{inst.target} dB target</span>
              </button>
            ))}
          </div>

          {/* Profiles section */}
          {profiles.length > 0 && (
            <div class="gs-profiles">
              <h4 class="gs-section-title">Saved Profiles</h4>
              <div class="gs-profile-list">
                {profiles.map((p) => (
                  <div key={p.id} class="gs-profile-row">
                    <span class="gs-profile-name">{p.name}</span>
                    <div class="gs-profile-actions">
                      <button class="gs-btn-sm" onClick={() => {
                        sendGainStaging({ action: "load_profile", name: p.name });
                        closeGainStaging();
                      }}>Load</button>
                      <button class="gs-btn-sm gs-btn-danger" onClick={() => {
                        sendGainStaging({ action: "delete_profile", name: p.name });
                        setProfiles((prev) => prev.filter((x) => x.name !== p.name));
                      }}>Delete</button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}

          <div class="gs-step-actions">
            <button class="gs-btn gs-btn-secondary" onClick={handleCancel}>Cancel</button>
            <button class="gs-btn gs-btn-primary" onClick={handleStartMeasure}>
              Measure Level
            </button>
          </div>
        </div>
      )}

      {/* Step 2: Level Measurement */}
      {step === 2 && (
        <div class="gs-measure">
          <div class="gs-measure-header">
            <button class="gs-btn-secondary gs-btn-sm" onClick={() => {
              if (countdownRef.current) clearInterval(countdownRef.current);
              setMeasurePhase("idle");
              setProgress(0);
              setPeakDb(-60);
              setCurrentDb(0);
              setStep(1);
            }}>← Back</button>
            <span class="gs-measure-instrument">{instrument?.icon} {instrument?.name}</span>
          </div>

          <div class="gs-meter-area">
            {/* Vertical level meter */}
            <div class="gs-meter-wrapper">
              <div class="gs-meter-track">
                {/* Green zone: -20 to 0 dBFS */}
                <div class="gs-meter-zone gs-zone-green" style={{ top: "66.7%" }}></div>
                {/* Yellow zone: -40 to -20 dBFS */}
                <div class="gs-meter-zone gs-zone-yellow" style={{ top: "33.3%" }}></div>
                {/* Red zone: below -40 dBFS */}
                <div class="gs-meter-zone gs-zone-red" style={{ top: "0%" }}></div>

                {/* Current level indicator */}
                <div
                  class="gs-meter-level"
                  style={{ bottom: `${Math.max(0, Math.min(100, ((currentDb + 60) / 60) * 100))}%` }}
                />

                {/* Peak indicator dot */}
                <div
                  class="gs-meter-peak"
                  style={{ bottom: `${Math.max(0, Math.min(100, ((peakDb + 60) / 60) * 100))}%` }}
                />

                {/* dBFS scale */}
                <div class="gs-meter-scale">
                  <span class="gs-scale-label" style={{ top: "0%" }}>0</span>
                  <span class="gs-scale-label" style={{ top: "33.3%" }}>-20</span>
                  <span class="gs-scale-label" style={{ top: "66.7%" }}>-40</span>
                  <span class="gs-scale-label gs-scale-min" style={{ top: "100%" }}>-60</span>
                </div>
              </div>

              {/* dBFS readout */}
              <div class="gs-dbfs-readout">
                <span class="gs-dbfs-value">{currentDb > -59 ? currentDb.toFixed(1) : "---"}</span>
                <span class="gs-dbfs-unit">dBFS</span>
              </div>
            </div>

            {/* Phase indicator */}
            <div class="gs-phase-area">
              {measurePhase === "countdown" && (
                <div class="gs-countdown">
                  <span class="gs-countdown-text">Hold for 5s</span>
                  <div class="gs-progress-bar">
                    <div class="gs-progress-fill" style={{ width: `${progress}%` }} />
                  </div>
                </div>
              )}
              {measurePhase === "playing" && (
                <div class="gs-playing">
                  <span class="gs-playing-dot" />
                  <span class="gs-playing-text">Playing…</span>
                </div>
              )}
            </div>

            <div class="gs-step-actions">
              <button class="gs-btn gs-btn-danger" onClick={handleCancel}>Cancel</button>
              <button
                class="gs-btn gs-btn-primary"
                disabled={measurePhase !== "done"}
                onClick={() => setStep(3)}
              >
                {measurePhase === "done" ? "View Results" : measurePhase === "playing" ? "Measuring…" : "Hold for 5s"}
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Step 3: Results & Save */}
      {step === 3 && measuredAvg !== null && (
        <div class="gs-results">
          <div class="gs-results-header">
            <button class="gs-btn-secondary gs-btn-sm" onClick={() => {
              setStep(2);
              setMeasurePhase("idle");
              setProgress(0);
              setPeakDb(-60);
              setCurrentDb(0);
            }}>← Retake</button>
            <h3 class="gs-step-title">Measurement Results</h3>
          </div>

          <div class="gs-results-grid">
            <div class="gs-result-card">
              <span class="gs-result-label">Average Level</span>
              <span class="gs-result-value gs-value-primary">{measuredAvg.toFixed(1)} dBFS</span>
            </div>
            <div class="gs-result-card">
              <span class="gs-result-label">Peak Level</span>
              <span class="gs-result-value gs-value-warning">{measuredPeak?.toFixed(1) ?? "---"} dBFS</span>
            </div>
            <div class="gs-result-card gs-result-card-cta">
              <span class="gs-result-label">Recommended Gain</span>
              <span class="gs-result-value gs-value-cta">{recommendedGain !== null ? (recommendedGain >= 0 ? "+" : "") : "---"}{recommendedGain?.toFixed(1) ?? "---"} dB</span>
            </div>
          </div>

          {/* Visual comparison bars */}
          <div class="gs-comparison">
            <div class="gs-comparison-row">
              <span class="gs-comparison-label">Your signal</span>
              <div class="gs-bar-track">
                <div class="gs-bar-fill gs-bar-current" style={{ width: `${Math.max(0, Math.min(100, ((measuredAvg + 60) / 60) * 100))}%` }} />
              </div>
            </div>
            <div class="gs-comparison-row">
              <span class="gs-comparison-label">Target</span>
              <div class="gs-bar-track">
                <div class="gs-bar-fill gs-bar-target" style={{ width: `${Math.max(0, Math.min(100, ((instrument?.target ?? -10) + 60) / 60 * 100))}%` }} />
              </div>
            </div>
          </div>

          <div class="gs-step-actions gs-results-actions">
            <button class="gs-btn gs-btn-secondary" onClick={handleStartOver}>Start Over</button>
            <button class="gs-btn gs-btn-secondary" onClick={closeGainStaging}>Close</button>
            {recommendedGain !== null && (
              <button class="gs-btn gs-btn-primary" onClick={handleApply}>Apply Gain</button>
            )}
            <button
              class="gs-btn gs-btn-outline"
              onClick={() => setShowProfileInput(true)}
            >
              Save as Profile
            </button>
          </div>

          {/* Profile name input */}
          {showProfileInput && (
            <div class="gs-profile-input">
              <input
                class="gs-input"
                type="text"
                placeholder="Profile name…"
                value={profileName}
                onInput={(e) => setProfileName((e.target as HTMLInputElement).value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleSaveProfile();
                }}
                autoFocus
              />
              <button class="gs-btn gs-btn-primary gs-btn-sm" onClick={handleSaveProfile}>Save</button>
              <button class="gs-btn gs-btn-secondary gs-btn-sm" onClick={() => setShowProfileInput(false)}>Cancel</button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

let gsReqId = 1000;
/** Send a gain_staging request in the standard `{type:"req", id, cmd, args}` envelope. */
function sendGainStaging(msg: Record<string, unknown>) {
  const w = bridge.socket;
  if (!w || w.readyState !== WebSocket.OPEN) return;
  const { action, ...rest } = msg as { action: string; [k: string]: unknown };
  try {
    w.send(JSON.stringify({ type: "req", id: gsReqId++, cmd: "gain_staging", args: { action, ...rest } }));
  } catch { /* ignore */ }
}
