import { useEffect, useRef, useState } from "react";
import { capabilityList, summaryText } from "../../lib/devices";
import { api } from "../../lib/ipc";
import type { DeviceInfo, TextScroll } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

/** What "Scroll a test text" sends: short, white, readable at its pace, once. */
export const TEST_TEXT: TextScroll = { text: "LaunchDeck", color: "#ffffff", speed: 10, direction: "left", looping: false };

/** The brightness is sent once the slider rests this long, not at every step of a drag. */
const BRIGHTNESS_PAUSE_MS = 250;

/** A connected device in Settings > Devices: one line about it, Sleep or Wake, the test text, what it can do. */
export function DeviceSettings({ device }: { device: DeviceInfo }) {
  const attempt = useStore((s) => s.attempt);
  const { id, name } = device.identity;
  const features = device.features;
  const details = device.details;

  return (
    <section className="device-settings" aria-label={`${name} settings`}>
      <h3>{summaryText(device)}</h3>
      {(features?.sleep || features?.scrollingText) && (
        <div className="row">
          {features.sleep && (
            <button type="button" className="btn small" onClick={() => void attempt(() => api.setDeviceSleep(id, !details?.asleep))}>
              {details?.asleep ? "Wake" : "Sleep"}
            </button>
          )}
          {features.scrollingText &&
            (details?.scrolling ? (
              <button type="button" className="btn small" onClick={() => void attempt(() => api.scrollDeviceText(id, null))}>
                Stop the text
              </button>
            ) : (
              <button type="button" className="btn small" onClick={() => void attempt(() => api.scrollDeviceText(id, TEST_TEXT))}>
                Scroll a test text
              </button>
            ))}
        </div>
      )}
      <ul className="capabilities" aria-label={`What the ${name} can do`}>
        {capabilityList(device).map((words) => (
          <li key={words}>{words}</li>
        ))}
      </ul>
    </section>
  );
}

/**
 * One brightness for every device that has it: the saved preference, or the device's own level. It goes to Rust
 * through `setDeviceBrightness` alone, which saves it; the other preferences are never sent with it.
 */
export function BrightnessSetting({ devices }: { devices: readonly DeviceInfo[] }) {
  const attempt = useStore((s) => s.attempt);
  const saved = useStore((s) => s.workspace?.preferences.deviceBrightness);
  const read = devices.find((d) => d.details?.brightness != null)?.details?.brightness;
  const [draft, setDraft] = useState(saved ?? read ?? 100);
  // The value the slider rests at, sent after a pause, or as the panel closes.
  const waiting = useRef<{ timer: ReturnType<typeof setTimeout>; flush: () => void } | null>(null);
  const own = saved === undefined;

  useEffect(() => {
    if (saved !== undefined) setDraft(saved);
  }, [saved]);
  useEffect(() => () => waiting.current?.flush(), []);

  const send = (percent: number | null) => {
    if (waiting.current) clearTimeout(waiting.current.timer);
    waiting.current = null;
    void attempt(() => api.setDeviceBrightness(percent));
  };
  const slide = (percent: number) => {
    setDraft(percent);
    if (waiting.current) clearTimeout(waiting.current.timer);
    waiting.current = { timer: setTimeout(() => send(percent), BRIGHTNESS_PAUSE_MS), flush: () => send(percent) };
  };

  return (
    <fieldset className="device-brightness">
      <legend>Brightness</legend>
      <p className="hint">For every Launchpad that can change it. LaunchDeck gives each its own level back when it lets go of it.</p>
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={own} onChange={(e) => send(e.currentTarget.checked ? null : draft)} />
          Device's own level
        </label>
        <input
          type="range"
          min={0}
          max={100}
          step={1}
          aria-label="Brightness"
          disabled={own}
          value={draft}
          onChange={(e) => slide(Number(e.currentTarget.value))}
        />
        <output>{own ? "" : `${draft}%`}</output>
      </div>
    </fieldset>
  );
}
