import { useState } from "react";
import { DeviceStatusLine } from "../../components/DeviceStatusLine";
import { api } from "../../lib/ipc";
import type { DeviceInfo } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

/** Settings > Devices: each device's status in words, Look again, Connect and Disconnect. */
export function DevicesPanel() {
  const devices = useStore((s) => s.state?.devices ?? null);
  const attempt = useStore((s) => s.attempt);
  const [looking, setLooking] = useState(false);
  if (!devices) return null;

  return (
    <section className="panel" aria-labelledby="devices-title">
      <div className="panel-head">
        <div>
          <h2 id="devices-title">Devices</h2>
          <p className="hint">Plug in a Launchpad and LaunchDeck connects it within a few seconds. If it does not show up, press Look again.</p>
        </div>
        <button
          type="button"
          className="btn"
          disabled={looking}
          onClick={() => {
            setLooking(true);
            void attempt(() => api.rescanDevices()).finally(() => setLooking(false));
          }}
        >
          {looking ? "Looking…" : "Look again"}
        </button>
      </div>
      <ul className="device-list big" aria-label="Devices">
        {devices.map((d) => (
          <li key={d.identity.id}>
            <DeviceStatusLine device={d} />
            <span className="grow" />
            <DeviceButton device={d} />
          </li>
        ))}
      </ul>
      {!devices.some((d) => d.identity.kind === "hardware") && <p className="hint">No Launchpad found. The simulator works the same way.</p>}
      <details className="tech-details">
        <summary>Technical details</summary>
        <p className="hint">
          A Launchpad is switched to its Programmer Mode while connected, and back when disconnected. LaunchDeck looks for its MIDI port: LPX MIDI on a
          Launchpad X, LPMiniMK3 MIDI on a Launchpad Mini MK3.
        </p>
        <ul className="plain">
          {devices.map((d) => (
            <li key={d.identity.id}>
              <code>{d.identity.id}</code> · {d.identity.model} · {technical(d)}
            </li>
          ))}
        </ul>
      </details>
    </section>
  );
}

function DeviceButton({ device }: { device: DeviceInfo }) {
  const attempt = useStore((s) => s.attempt);
  const id = device.identity.id;
  switch (device.status) {
    case "connected":
      return (
        <button type="button" className="btn small" aria-label={`Disconnect ${device.identity.name}`} onClick={() => void attempt(() => api.disconnectDevice(id))}>
          Disconnect
        </button>
      );
    case "available":
    case "failed":
      return (
        <button type="button" className="btn small primary" aria-label={`Connect ${device.identity.name}`} onClick={() => void attempt(() => api.connectDevice(id))}>
          Connect
        </button>
      );
    default:
      // Unplugged comes back by itself; an unsupported model is never opened.
      return null;
  }
}

/** Capabilities as they matter to someone debugging the layout or the lights. */
function technical(d: DeviceInfo): string {
  const c = d.capabilities;
  return [
    c.grid ? `${c.grid.width}×${c.grid.height} grid` : "no grid",
    `${c.aux.top} top, ${c.aux.side} side${c.aux.logo ? ", logo" : ""}`,
    c.rgb ? "RGB" : "single colour",
    c.velocity ? "velocity" : "no velocity",
    c.animations.pulse ? "native pulse" : "software pulse",
  ].join(" · ");
}
