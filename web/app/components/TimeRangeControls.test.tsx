import { Children, isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("./DateTimePicker", () => ({
  default: function MockDateTimePicker() {
    return null;
  },
}));

import DateTimePicker from "./DateTimePicker";
import { TimeRangeControls } from "./TimeRangeControls";
import { getPresetRangeAt, resolveEffectiveRange, type TimeRange } from "../lib/time-range";

function findElements(
  node: ReactNode,
  predicate: (element: ReactElement<Record<string, unknown>>) => boolean,
): ReactElement<Record<string, unknown>>[] {
  const found: ReactElement<Record<string, unknown>>[] = [];
  if (!isValidElement<Record<string, unknown>>(node)) return found;
  if (predicate(node)) found.push(node);
  for (const child of Children.toArray(node.props.children as ReactNode)) {
    found.push(...findElements(child, predicate));
  }
  return found;
}

function renderedControls(
  displayedRange: TimeRange,
  onRangeChange: (range: TimeRange) => void,
) {
  return TimeRangeControls({
    displayedRange,
    rangeIsSettling: false,
    presetLabels: {
      "1m": "1m",
      "5m": "5m",
      "1h": "1h",
      "6h": "6h",
      "12h": "12h",
      "24h": "24h",
      "7d": "7d",
      "30d": "30d",
    },
    browserTimeTemplate: "Browser time ({offset})",
    utcStorageLabel: "UTC",
    rangeUpdatingLabel: "Updating",
    onPresetClick: vi.fn(),
    onRangeChange,
  });
}

describe("TimeRangeControls", () => {
  const liveDisplayedRange = resolveEffectiveRange(
    getPresetRangeAt(5, "5m", Date.parse("2026-09-24T05:00:00Z")),
    "2026-09-24T05:11:00Z",
    Date.parse("2026-09-24T05:00:00Z"),
  );

  it("wires the start picker to the currently displayed live endpoint", () => {
    const onRangeChange = vi.fn();
    const tree = renderedControls(liveDisplayedRange, onRangeChange);
    const pickers = findElements(tree, (element) => element.type === DateTimePicker);

    expect(pickers).toHaveLength(2);
    const onChange = pickers[0]?.props.onChange as ((date: Date) => void) | undefined;
    expect(onChange).toBeTypeOf("function");
    onChange?.(new Date("2026-09-24T05:08:00Z"));

    expect(onRangeChange).toHaveBeenCalledWith({
      start: new Date("2026-09-24T05:08:00Z"),
      end: new Date("2026-09-24T05:11:00Z"),
      preset: "custom",
    });
  });

  it("wires the end picker to preserve the currently displayed live start", () => {
    const onRangeChange = vi.fn();
    const tree = renderedControls(liveDisplayedRange, onRangeChange);
    const pickers = findElements(tree, (element) => element.type === DateTimePicker);
    const onChange = pickers[1]?.props.onChange as ((date: Date) => void) | undefined;

    expect(onChange).toBeTypeOf("function");
    onChange?.(new Date("2026-09-24T05:10:00Z"));

    expect(onRangeChange).toHaveBeenCalledWith({
      start: new Date("2026-09-24T05:06:00Z"),
      end: new Date("2026-09-24T05:10:00Z"),
      preset: "custom",
    });
  });

  it("wires both pickers to non-reversing custom transitions", () => {
    const onRangeChange = vi.fn();
    const tree = renderedControls(liveDisplayedRange, onRangeChange);
    const pickers = findElements(tree, (element) => element.type === DateTimePicker);
    const startChange = pickers[0]?.props.onChange as ((date: Date) => void);
    const endChange = pickers[1]?.props.onChange as ((date: Date) => void);

    startChange(new Date("2026-09-24T05:12:00Z"));
    endChange(new Date("2026-09-24T05:05:00Z"));

    const [startResult, endResult] = onRangeChange.mock.calls.map(
      ([range]) => range as TimeRange,
    );
    expect(startResult.start.getTime()).toBe(startResult.end.getTime());
    expect(endResult.start.getTime()).toBe(endResult.end.getTime());
  });
});
