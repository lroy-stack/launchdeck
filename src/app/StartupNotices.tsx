// What Rust had to tell about this start: a saved workspace that could not be used was kept as a backup and
// LaunchDeck started fresh. A banner per notice stays until the person dismisses it.

import { useState } from "react";
import { api } from "../lib/ipc";
import type { StartupNotice } from "../lib/schema";
import { useStore } from "../stores/runtime";

const TEXT: Record<StartupNotice["kind"], string> = {
  workspaceDamaged: "Your surfaces could not be opened, so LaunchDeck started fresh. Your file was kept as workspace.json.bak.",
  workspaceTooNew: "Your surfaces were saved by a newer LaunchDeck, so this one started fresh. Your file was kept as workspace.json.bak.",
};

export function StartupNotices() {
  const notices = useStore((s) => s.notices);
  const attempt = useStore((s) => s.attempt);
  const [dismissed, setDismissed] = useState<number[]>([]);

  return notices.map((notice, i) =>
    dismissed.includes(i) ? null : (
      <div key={`${notice.kind}:${notice.backup}`} className="startup-notice" role="alert">
        <p>{TEXT[notice.kind]}</p>
        <div className="row">
          <button type="button" className="btn small" onClick={() => void attempt(() => api.revealDataFolder())}>
            Show folder
          </button>
          <button type="button" className="btn small" onClick={() => setDismissed([...dismissed, i])}>
            Dismiss
          </button>
        </div>
        <details className="tech-details">
          <summary>Details</summary>
          <code>{notice.backup}</code>
        </details>
      </div>
    ),
  );
}
