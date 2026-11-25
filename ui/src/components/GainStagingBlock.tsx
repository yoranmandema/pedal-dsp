import { state, openGainStaging } from "../state";
import "./gain-staging.css";

// Instrument type icons for the block renderer
const INSTRUMENT_ICONS: Record<string, string> = {
  electric_guitar: "🎸",
  bass: "🎸",
  acoustic_guitar: "🪕",
  microphone: "🎙️",
  line_in: "🔌",
  active_pickup: "🎸",
  passive_pickup: "🎸",
  other: "⚙️",
};

// Default instrument when none is set
const DEFAULT_INSTRUMENT = "electric_guitar";

interface GainStagingBlockProps {
  channel: string;
  block: { id: string; type: string; name: string; params: Record<string, number> };
}

export function GainStagingBlock({ block: _block, channel: _channel }: GainStagingBlockProps) {
  const s = state.value;
  const gs = s?.gainStaging;
  if (!gs) return null;

  const instrument = gs.instrument ?? DEFAULT_INSTRUMENT;
  const icon = INSTRUMENT_ICONS[instrument] ?? "⚙️";
  const currentDb = gs.currentDb ?? -60;
  const peakDb = gs.peakDb ?? -60;

  // Calculate level percentage for meter (-60 to 0 dBFS → 0–100%)
  const levelPct = Math.max(0, Math.min(100, ((currentDb + 60) / 60) * 100));
  const peakPct = Math.max(0, Math.min(100, ((peakDb + 60) / 60) * 100));

  // Determine meter color based on level
  const meterColor = currentDb < -40 ? "red" : currentDb < -20 ? "yellow" : "green";

  const openModalFor = () => {
    openGainStaging();
  };

  return (
    <div class="gs-block card" onClick={openModalFor} title={`Gain Staging: ${instrument} — ${currentDb.toFixed(1)} dBFS`}>
      {/* Header with icon and label */}
      <div class="gs-block-header">
        <span class="gs-block-icon">{icon}</span>
        <span class="gs-block-label">GS</span>
      </div>

      {/* Vertical level meter on the left */}
      <div class="gs-block-meter">
        <div class="gs-block-meter-track">
          {/* Current level bar */}
          <div
            class={`gs-block-meter-fill gs-meter-${meterColor}`}
            style={{ height: `${levelPct}%` }}
          />
          {/* Peak indicator */}
          <div
            class="gs-block-meter-peak"
            style={{ bottom: `${peakPct}%` }}
          />
        </div>
      </div>

      {/* Current dBFS reading */}
      <div class="gs-block-dbfs">
        <span class="gs-block-dbfs-value">{currentDb > -59 ? currentDb.toFixed(1) : "---"}</span>
        <span class="gs-block-dbfs-unit">dB</span>
      </div>
    </div>
  );
}
