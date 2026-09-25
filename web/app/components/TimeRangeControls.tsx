import DateTimePicker from "./DateTimePicker";
import {
  customizeDisplayedRange,
  formatBrowserTimeZone,
  type PresetKey,
  type TimeRange,
} from "../lib/time-range";

type PresetButtonKey = Exclude<PresetKey, "custom">;

const PRESET_CONFIG: readonly { key: PresetButtonKey; minutes: number }[] = [
  { key: "1m", minutes: 1 },
  { key: "5m", minutes: 5 },
  { key: "1h", minutes: 60 },
  { key: "6h", minutes: 360 },
  { key: "12h", minutes: 720 },
  { key: "24h", minutes: 1_440 },
  { key: "7d", minutes: 10_080 },
  { key: "30d", minutes: 43_200 },
];

interface TimeRangeControlsProps {
  displayedRange: TimeRange;
  rangeIsSettling: boolean;
  presetLabels: Readonly<Record<PresetButtonKey, string>>;
  browserTimeTemplate: string;
  utcStorageLabel: string;
  rangeUpdatingLabel: string;
  onPresetClick: (minutes: number, key: PresetKey) => void;
  onRangeChange: (range: TimeRange) => void;
}

export function TimeRangeControls({
  displayedRange,
  rangeIsSettling,
  presetLabels,
  browserTimeTemplate,
  utcStorageLabel,
  rangeUpdatingLabel,
  onPresetClick,
  onRangeChange,
}: TimeRangeControlsProps) {
  const timeZoneLabel = formatBrowserTimeZone(displayedRange.end);

  return (
    <>
      <div className="time-controls">
        {PRESET_CONFIG.map(({ key, minutes }) => (
          <button
            key={key}
            type="button"
            className={`preset-btn ${displayedRange.preset === key ? "active" : ""}`}
            onClick={() => onPresetClick(minutes, key)}
          >
            {presetLabels[key]}
          </button>
        ))}
        <div style={{ width: 1, height: 24, background: "var(--border-subtle)", margin: "0 4px" }} />
        <DateTimePicker
          value={displayedRange.start}
          onChange={(date) => onRangeChange(
            customizeDisplayedRange(displayedRange, "start", date),
          )}
        />
        <span style={{ color: "var(--text-muted)", fontSize: 13 }}>~</span>
        <DateTimePicker
          value={displayedRange.end}
          onChange={(date) => onRangeChange(
            customizeDisplayedRange(displayedRange, "end", date),
          )}
        />
      </div>
      <div className="chart-time-contract" aria-live="polite">
        <span>{browserTimeTemplate.replace("{offset}", timeZoneLabel)}</span>
        <span>{utcStorageLabel}</span>
        {rangeIsSettling && <span>{rangeUpdatingLabel}</span>}
      </div>
    </>
  );
}
