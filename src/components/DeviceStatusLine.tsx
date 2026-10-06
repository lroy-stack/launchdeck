import { statusText, statusTone } from "../lib/devices";
import type { DeviceInfo } from "../lib/schema";

/**
 * A device's name, its status in words and, when something went wrong, the
 * sentence Rust gave. A status dot and a text column, for a flex row.
 */
export function DeviceStatusLine({ device }: { device: DeviceInfo | null }) {
  if (!device) {
    return (
      <>
        <span className="status-dot" aria-hidden />
        <span className="device-text">
          <b>No device</b>
          <small>Plug in a Launchpad</small>
        </span>
      </>
    );
  }
  return (
    <>
      <span className={`status-dot ${statusTone(device)}`} aria-hidden />
      <span className="device-text">
        <b>{device.identity.name}</b>
        <small>{statusText(device)}</small>
        {device.problem && <small className="device-problem">{device.problem}</small>}
      </span>
    </>
  );
}
