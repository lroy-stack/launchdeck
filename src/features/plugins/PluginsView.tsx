import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/ipc";
import type { PluginConfig, PluginInfo, PluginLog, PluginManifest, PluginsOverview, ProblemKind, SecretStoreStatus } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { LegacySecretsNotice, NetworkAccess, Secrets } from "./Access";
import { LegacySecret } from "./LegacySecret";
import { folderName, problemText } from "./text";

const PERMISSION_NOTES: Record<string, string> = {
  automation: "Sends keystrokes and opens apps on your behalf.",
  network: "Makes network requests.",
  filesystem: "Reads or writes files.",
  shell: "Runs shell commands.",
  accessibility: "Uses OS accessibility APIs.",
  midi: "Talks to MIDI hardware.",
};

export function PluginsView() {
  const builtIn = useStore((s) => s.plugins);
  const attempt = useStore((s) => s.attempt);
  const toast = useStore((s) => s.toast);
  const refreshPlugins = useStore((s) => s.refreshPlugins);
  const [overview, setOverview] = useState<PluginsOverview | null>(null);

  const load = useCallback(async () => {
    await attempt(async () => setOverview(await api.plugins.list()));
  }, [attempt]);

  useEffect(() => {
    void load();
  }, [load]);

  // Every change goes through here: do it, re-read the host's answer, and let
  // the rest of the app (the action picker) see the new action catalogue.
  const change = useCallback(
    async (fn: () => Promise<unknown>) => {
      const ok = await attempt(fn);
      await load();
      await refreshPlugins();
      return ok;
    },
    [attempt, load, refreshPlugins],
  );

  const rescan = async () => {
    let summary = "";
    const ok = await change(async () => {
      const r = await api.plugins.rescan();
      summary = `Scan finished: ${r.loaded} new, ${r.reloaded} reloaded, ${r.removed} removed, ${r.issues} with problems.`;
    });
    if (ok) toast(summary);
  };

  const installed = overview?.plugins ?? [];
  const installedIds = new Set(installed.map((i) => i.plugin.id));
  const bundled = builtIn.filter((p) => !p.external && !installedIds.has(p.id));

  return (
    <div className="page-scroll">
      <header className="page-head">
        <h1>Plugins</h1>
        <p>
          Plugins add semantic actions and publish events. They never see MIDI notes or pad positions: a plugin says <code>example.hello.completed</code>, and the
          surface decides which pad lights up. Installed plugins run in a sandbox with no access to files, shell or devices. One that needs the network says
          exactly which addresses, and reaches none of them until you allow it here.
        </p>
      </header>

      <section className="panel plugin-folders">
        <div className="field-head">
          <h4>Installed plugins</h4>
          <button className="btn small" onClick={() => void rescan()}>
            Rescan
          </button>
        </div>
        {overview && overview.folders.length > 0 ? (
          <p className="hint">
            LaunchDeck looks for one folder per plugin, each with a <code>plugin.json</code> and a <code>.wasm</code> file, in:{" "}
            {overview.folders.map((f, i) => (
              <span key={f}>
                {i > 0 && ", "}
                <code>{f}</code>
              </span>
            ))}
            .
          </p>
        ) : (
          <p className="hint">Plugins are looked up in the desktop app only.</p>
        )}
        {overview && overview.issues.length > 0 && (
          <ul className="plain issues">
            {overview.issues.map((i) => (
              <li key={i.dir}>
                <span className="pill warn">skipped</span>
                <code>{folderName(i.dir)}</code>
                <ProblemNote kind={i.kind} error={i.error} dir={i.dir} />
              </li>
            ))}
          </ul>
        )}
        {overview && installed.length === 0 && overview.issues.length === 0 && <p className="hint">Nothing installed yet. See docs/plugin-sdk/getting-started.md.</p>}
      </section>

      {overview && <LegacySecretsNotice overview={overview} change={change} />}

      <div className="cards wide">
        {installed.map((info) => (
          <InstalledCard key={info.plugin.id} info={info} store={overview?.secretStore ?? NO_STORE} change={change} />
        ))}
        {bundled.map((p) => (
          <article key={p.id} className="card plugin">
            <header>
              <h3>{p.name}</h3>
              <span className="pill">v{p.version}</span>
              <span className="pill ghost">built in</span>
            </header>
            <p>{p.description}</p>
            <ActionList plugin={p} />
            <StateList plugin={p} />
            <h4>Permissions</h4>
            {p.permissions.length === 0 ? (
              <p className="hint">None requested.</p>
            ) : (
              <ul className="plain">
                {p.permissions.map((perm) => (
                  <li key={perm}>
                    <span className="pill warn">{perm}</span>
                    <span>{PERMISSION_NOTES[perm] ?? ""}</span>
                  </li>
                ))}
              </ul>
            )}
          </article>
        ))}
      </div>
      <p className="hint foot">
        Built-in plugins are part of LaunchDeck and declare what they use. Installed plugins can only use the small contract the host offers. Network access is the
        one thing an installed plugin can ask for, by exact address, and it is off until you allow it. See docs/plugins/permissions.md.
      </p>
    </div>
  );
}

function ActionList({ plugin }: { plugin: PluginManifest }) {
  // Installed plugins namespace their own action ids ("counter.increment"), so
  // the card's id line already says which plugin they belong to.
  const prefix = plugin.external ? "" : `${plugin.id}.`;
  return (
    <>
      <h4>Actions</h4>
      <ul className="plain">
        {plugin.actions.map((a) => (
          <li key={a.id}>
            <code>
              {prefix}
              {a.id}
              {a.params.length > 0 && `(${a.params.map((x) => `${x.name}${x.required ? "" : "?"}: ${x.type}`).join(", ")})`}
            </code>
            <span>{a.description}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

function StateList({ plugin }: { plugin: PluginManifest }) {
  if (plugin.states.length === 0) return null;
  return (
    <>
      <h4>State it publishes</h4>
      <ul className="plain">
        {plugin.states.map((s) => (
          <li key={s.key}>
            <code>{s.key}</code>
            <span>{s.values.join(" · ")}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

/** Until the host has answered, nothing is said to be available. */
const NO_STORE: SecretStoreStatus = { kind: "unavailable", available: false, name: "", reason: "" };

function InstalledCard({ info, store, change }: { info: PluginInfo; store: SecretStoreStatus; change: (fn: () => Promise<unknown>) => Promise<boolean> }) {
  const p = info.plugin;
  const ext = p.external;
  const status = ext?.status ?? "disabled";
  const [logs, setLogs] = useState<PluginLog[] | null>(null);
  const attempt = useStore((s) => s.attempt);

  const toggleLogs = async () => {
    if (logs) return setLogs(null);
    await attempt(async () => setLogs(await api.plugins.logs(p.id)));
  };
  // Keep an open log view current after the plugin does something.
  const refreshLogs = async () => {
    if (logs) await attempt(async () => setLogs(await api.plugins.logs(p.id)));
  };

  return (
    <article className={`card plugin ${status}`}>
      <header>
        <h3>{p.name}</h3>
        <span className="pill">v{p.version}</span>
        <span className="pill ghost">installed</span>
        <span className={`pill ${status === "enabled" ? "live" : status === "error" ? "warn" : "ghost"}`}>{status === "enabled" ? <><i />on</> : status}</span>
      </header>
      <p>{p.description}</p>
      <p className="hint">
        <code>{p.id}</code> by {ext?.author ?? "unknown"}.
        {status !== "error" && (
          <>
            {" "}
            Folder: <code>{info.dir}</code>
          </>
        )}
      </p>
      {status === "error" && <ProblemNote kind={ext?.kind} error={ext?.error} dir={info.dir} />}
      <div className="row wrap">
        {status === "error" ? (
          <button className="btn small primary" onClick={() => void change(() => api.plugins.reload(p.id)).then(refreshLogs)}>
            Reload
          </button>
        ) : (
          <button className="btn small" onClick={() => void change(() => api.plugins.setEnabled(p.id, !info.enabled)).then(refreshLogs)}>
            {info.enabled ? "Disable" : "Enable"}
          </button>
        )}
        {status === "error" && info.enabled && (
          <button className="btn small" onClick={() => void change(() => api.plugins.setEnabled(p.id, false)).then(refreshLogs)}>
            Disable
          </button>
        )}
        <button className="btn small" onClick={() => void toggleLogs()}>
          {logs ? "Hide log" : "Show log"}
        </button>
      </div>
      <LegacySecret info={info} store={store} change={change} />
      <NetworkAccess info={info} change={change} />
      <ActionList plugin={p} />
      {ext && ext.events.length > 0 && (
        <>
          <h4>Events it publishes</h4>
          <ul className="plain">
            {ext.events.map((e) => (
              <li key={e.type}>
                <code>
                  {p.id}/{e.type}
                </code>
                <span>{e.description}</span>
              </li>
            ))}
          </ul>
        </>
      )}
      {info.config.length > 0 && (
        <>
          <h4>Settings</h4>
          {info.config.map((c) => (
            <ConfigField key={`${p.id}/${c.key}/${c.isSet}`} pluginId={p.id} item={c} change={change} />
          ))}
        </>
      )}
      <Secrets info={info} store={store} change={change} />
      {logs && <LogView logs={logs} />}
    </article>
  );
}

/** One sentence for what went wrong; the host's own words and the folder wait under Details. */
function ProblemNote({ kind, error, dir }: { kind?: ProblemKind; error?: string; dir: string }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="plugin-problem">
      <p className="field-error">{problemText(kind)}</p>
      <details onToggle={(e) => setOpen(e.currentTarget.open)}>
        <summary>Details</summary>
        {open && (
          <p className="hint">
            {error && <span>{error}</span>} Folder: <code>{dir}</code>
          </p>
        )}
      </details>
    </div>
  );
}

function ConfigField({ pluginId, item, change }: { pluginId: string; item: PluginConfig; change: (fn: () => Promise<unknown>) => Promise<boolean> }) {
  const initial = String(item.value ?? item.default ?? "");
  const [text, setText] = useState(initial);
  const name = item.label !== item.key ? <b>{item.label}</b> : <code>{item.key}</code>;
  // A credential in the format of manifests before schema 4. LaunchDeck takes
  // no new value for it and hands none to the plugin, so there is nothing to
  // type. A plain value an earlier version left is on the Legacy insecure
  // secret card.
  if (item.secret) {
    return (
      <div className="field">
        <span>
          {name} {item.description}
        </span>
        <small className="hint warn">
          A credential in an obsolete format. LaunchDeck no longer accepts or uses a value for it; the plugin needs a version that declares it as a secret, kept in
          secure storage.
        </small>
      </div>
    );
  }
  const save = (value: string | number | boolean | null) => void change(() => api.plugins.setConfig(pluginId, item.key, value));

  const commit = () => {
    const trimmed = text.trim();
    if (trimmed === "") return;
    if (item.type === "number") {
      const n = Number(trimmed);
      if (Number.isFinite(n)) save(n);
      return;
    }
    if (trimmed === initial) return;
    save(trimmed);
  };

  return (
    <label className="field">
      <span>
        {name} {item.description}
      </span>
      {item.type === "boolean" ? (
        <input type="checkbox" checked={item.value === true} onChange={(e) => save(e.target.checked)} />
      ) : (
        <div className="row">
          <input
            type={item.type === "number" ? "number" : "text"}
            value={text}
            autoComplete="off"
            onChange={(e) => setText(e.target.value)}
            onBlur={commit}
            onKeyDown={(e) => e.key === "Enter" && commit()}
          />
          {item.value !== undefined && item.value !== null && (
            <button className="btn small" type="button" onClick={() => save(null)}>
              Clear
            </button>
          )}
        </div>
      )}
    </label>
  );
}

function LogView({ logs }: { logs: PluginLog[] }) {
  if (logs.length === 0) return <p className="hint">Nothing logged yet.</p>;
  return (
    <pre className="command-preview plugin-log">
      {logs
        .slice(-60)
        .map((l) => `${new Date(l.ts).toLocaleTimeString()} ${l.level.padEnd(5)} ${l.origin === "host" ? "[host] " : ""}${l.message}`)
        .join("\n")}
    </pre>
  );
}
