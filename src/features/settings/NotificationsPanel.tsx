import { api } from "../../lib/ipc";
import { useStore } from "../../stores/runtime";

/** Settings > Notifications: Agent Control's one switch for what may interrupt the person. */
export function NotificationsPanel() {
  const on = useStore((s) => s.agents.settings.notifications);
  const attempt = useStore((s) => s.attempt);
  return (
    <section className="panel" aria-labelledby="notifications-title">
      <div className="panel-head">
        <div>
          <h2 id="notifications-title">Notifications</h2>
          <p className="hint">Only four things interrupt you: an approval is needed, an agent fails, a long run completes, an adapter disconnects.</p>
        </div>
        <label className="check">
          <input type="checkbox" checked={on} onChange={(e) => void attempt(() => api.agents.updateSettings({ notifications: e.target.checked }))} />
          Enabled
        </label>
      </div>
    </section>
  );
}
