import { useEffect, useState } from "react";
import { api, errorMessage } from "../../lib/ipc";
import type { AppInfo } from "../../lib/schema";
import { ContextPanel } from "../context/ContextPanel";
import { AboutPanel } from "./AboutPanel";
import { DeveloperPanel } from "./DeveloperPanel";
import { DevicesPanel } from "./DevicesPanel";
import { NotificationsPanel } from "./NotificationsPanel";
import { PluginsPanel } from "./PluginsPanel";
import { SecurityPanel } from "./SecurityPanel";
import { WorkspacePanel } from "./WorkspacePanel";

/** The sections, in the order a person meets them; the list at the top leads to each. */
const SECTIONS = [
  { id: "settings-devices", title: "Devices" },
  { id: "settings-context", title: "Auto Context" },
  { id: "settings-notifications", title: "Notifications" },
  { id: "settings-plugins", title: "Plugins & permissions" },
  { id: "settings-security", title: "Security" },
  { id: "settings-workspace", title: "Workspace" },
  { id: "settings-developer", title: "Developer" },
  { id: "settings-about", title: "About" },
] as const;

type SectionId = (typeof SECTIONS)[number]["id"];

export function SettingsView() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [infoProblem, setInfoProblem] = useState<string | null>(null);

  // One read for About and Developer: the versions and the data folder do not change while the app runs.
  useEffect(() => {
    let live = true;
    api
      .appInfo()
      .then((i) => live && setInfo(i))
      .catch((e: unknown) => live && setInfoProblem(errorMessage(e)));
    return () => {
      live = false;
    };
  }, []);

  const body: Record<SectionId, React.ReactNode> = {
    "settings-devices": <DevicesPanel />,
    "settings-context": <ContextPanel />,
    "settings-notifications": <NotificationsPanel />,
    "settings-plugins": <PluginsPanel />,
    "settings-security": <SecurityPanel />,
    "settings-workspace": <WorkspacePanel />,
    "settings-developer": <DeveloperPanel info={info} />,
    "settings-about": <AboutPanel info={info} problem={infoProblem} />,
  };

  return (
    <div className="page-scroll settings">
      <header className="page-head">
        <h1>Settings</h1>
        <nav aria-label="Settings sections">
          <ul className="settings-nav">
            {SECTIONS.map((s) => (
              <li key={s.id}>
                <button type="button" className="link" onClick={() => document.getElementById(s.id)?.scrollIntoView?.({ block: "start" })}>
                  {s.title}
                </button>
              </li>
            ))}
          </ul>
        </nav>
      </header>
      {SECTIONS.map((s) => (
        <div key={s.id} id={s.id} className="settings-section">
          {body[s.id]}
        </div>
      ))}
    </div>
  );
}
