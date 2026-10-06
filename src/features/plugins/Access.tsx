// What a plugin asks of the network, what was decided, and its secrets.
//
// Everything shown here is what Rust reported: which destinations the plugin
// asks for, how far each one reaches, what the person decided, whether a secret
// holds a value. The interface decides nothing; it shows the exact destination
// and sends the person's answer.

import { useState } from "react";
import { ProblemText } from "../../components/Problem";
import { api } from "../../lib/ipc";
import type { GrantStatus, LegacySecrets, NetworkHost, PluginInfo, PluginSecret, PluginsOverview, Reach, SecretStoreStatus } from "../../lib/schema";
import { useStore } from "../../stores/runtime";

type Change = (fn: () => Promise<unknown>) => Promise<boolean>;

const REACH: Record<Reach, string> = {
  loopback: "This computer",
  local: "Your local network",
  internet: "The internet",
};

const STATUS: Record<GrantStatus, string> = {
  granted: "Granted",
  denied: "Denied",
  needsApproval: "Needs approval",
};

/** Where a secret is kept, in a sentence. Only for a store that can be used. */
const kept = (store: SecretStoreStatus): string =>
  store.kind === "memory" ? "Kept in memory only, and forgotten when LaunchDeck closes." : `Kept in your system's credential store (${store.name}).`;

export function StatusPill({ status }: { status: GrantStatus }) {
  return <span className={`pill grant ${status}`}>{STATUS[status]}</span>;
}

/** The name of the setting a destination comes from, as the person sees it in Settings below. */
function settingName(info: PluginInfo, key: string): string {
  return info.config.find((c) => c.key === key)?.label ?? key;
}

/** The network section of a plugin's card. Absent for a plugin that asks for nothing. */
export function NetworkAccess({ info, change }: { info: PluginInfo; change: Change }) {
  const net = info.network;
  if (!net) return null;
  const name = info.plugin.name;
  const waiting = net.hosts.filter((h) => h.status === "needsApproval");
  const decided = net.hosts.filter((h) => h.status !== "needsApproval");
  const decide = (origin: string, decision: "granted" | "denied" | null) => void change(() => api.plugins.setPermission(info.plugin.id, origin, decision));
  const failure = info.networkFailure;

  return (
    <section className="plugin-access" aria-label={`Network access for ${name}`}>
      <div className="field-head">
        <h4>Network access</h4>
        <StatusPill status={net.status} />
      </div>

      {waiting.length > 0 && (
        <div className="permission-prompt" role="group" aria-label={`${name} requests network access`}>
          <p>
            <b>{name}</b> requests network access to:
          </p>
          <ul className="plain hosts">
            {waiting.map((h) => (
              <HostLine key={h.origin} info={info} host={h} />
            ))}
          </ul>
          <p className="purpose">
            <span>Purpose</span> {net.purpose}
          </p>
          <p className="hint">
            It will be able to send requests to {waiting.length === 1 ? "this address" : "these addresses"} and read the answers, and to no other. You can take this back at any time.
          </p>
          {waiting.map((h) => (
            <div className="row" key={h.origin}>
              <button type="button" className="btn small" aria-label={`Deny ${h.origin}`} onClick={() => decide(h.origin, "denied")}>
                Deny
              </button>
              <button type="button" className="btn small primary" aria-label={`Allow ${h.origin}`} onClick={() => decide(h.origin, "granted")}>
                Allow
              </button>
              {waiting.length > 1 && <code>{h.origin}</code>}
            </div>
          ))}
        </div>
      )}

      {decided.length > 0 && (
        <ul className="plain hosts decided">
          {decided.map((h) => (
            <li key={h.origin}>
              <HostLine info={info} host={h} bare />
              <StatusPill status={h.status} />
              {h.status === "granted" ? (
                <button type="button" className="btn small" aria-label={`Revoke ${h.origin}`} onClick={() => decide(h.origin, null)}>
                  Revoke
                </button>
              ) : (
                <button type="button" className="btn small" aria-label={`Allow ${h.origin}`} onClick={() => decide(h.origin, "granted")}>
                  Allow
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {decided.length > 0 && waiting.length === 0 && (
        <p className="hint">
          <span className="purpose-word">Purpose:</span> {net.purpose}
        </p>
      )}

      {net.unresolved.map((u) => (
        <p className="hint warn" key={u.setting}>
          Nothing to ask for yet: the address comes from <b>{settingName(info, u.setting)}</b> under Settings. {u.reason}
        </p>
      ))}

      {net.stale.length > 0 && (
        <div className="stale">
          <p className="hint">Decisions about addresses this plugin no longer asks for. They allow nothing.</p>
          <ul className="plain hosts">
            {net.stale.map((s) => (
              <li key={s.origin}>
                <code>{s.origin}</code>
                <span className="hint">{s.decision === "granted" ? "was granted" : "was denied"}</span>
                <button type="button" className="btn small" aria-label={`Forget ${s.origin}`} onClick={() => decide(s.origin, null)}>
                  Forget
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {failure && (
        <ProblemText problem={{ message: sentence(failure.message), detail: failure.code }} lead={<b>Last request did not go through.</b>} role="status" />
      )}
    </section>
  );
}

const sentence = (text: string): string => {
  const t = text.trim();
  const capital = t.charAt(0).toUpperCase() + t.slice(1);
  return /[.!?]$/.test(capital) ? capital : `${capital}.`;
};

function HostLine({ info, host, bare }: { info: PluginInfo; host: NetworkHost; bare?: boolean }) {
  const body = (
    <>
      <code className="origin">{host.origin}</code>
      <span className="reach">{REACH[host.reach]}</span>
      {host.source.kind === "setting" && <span className="hint">from the setting “{settingName(info, host.source.key)}”</span>}
    </>
  );
  return bare ? <span className="host-line">{body}</span> : <li className="host-line">{body}</li>;
}

/** The secrets of a plugin: set, replace, remove. A value is typed here and never shown again. */
export function Secrets({ info, store, change }: { info: PluginInfo; store: SecretStoreStatus; change: Change }) {
  if (info.secrets.length === 0) return null;
  return (
    <section className="plugin-access" aria-label={`Secrets of ${info.plugin.name}`}>
      <div className="field-head">
        <h4>Secrets</h4>
        {!store.available && <span className="pill grant denied">Unavailable</span>}
      </div>
      {store.available ? (
        <p className="hint">A secret is used by LaunchDeck on the plugin's behalf and is never handed to the plugin. {kept(store)}</p>
      ) : (
        <div className="store-unavailable" role="note" aria-label="Why secrets are unavailable">
          <p>
            <b>Secrets cannot be saved on this computer.</b> {store.reason}
          </p>
          <p className="hint">
            LaunchDeck keeps a secret in the system's credential store or not at all: it is never written to a plain file. Until a credential store is available,
            what {info.plugin.name} does with {info.secrets.length === 1 ? "this secret" : "these secrets"} will not work. Everything else does.
          </p>
        </div>
      )}
      {info.secrets.map((s) => (
        <SecretField key={`${info.plugin.id}/${s.key}/${s.isSet}`} pluginId={info.plugin.id} secret={s} available={store.available} change={change} />
      ))}
    </section>
  );
}

function SecretField({ pluginId, secret, available, change }: { pluginId: string; secret: PluginSecret; available: boolean; change: Change }) {
  const [text, setText] = useState("");
  const id = `secret-${pluginId}-${secret.key}`;
  const save = () => {
    if (text === "" || !available) return;
    void change(() => api.plugins.setSecret(pluginId, secret.key, text)).then((ok) => ok && setText(""));
  };
  return (
    <div className="field secret">
      <div className="secret-head">
        <label htmlFor={id}>{secret.label}</label>
        {available && <span className={`pill ${secret.isSet ? "live" : "ghost"}`}>{secret.isSet ? "Set" : "Not set"}</span>}
      </div>
      <small className="hint" id={`${id}-about`}>
        {secret.description} {secret.origin ? <>Only ever sent to <code>{secret.origin}</code>.</> : "It has no destination until the address above is set."}
      </small>
      <div className="row">
        <input
          id={id}
          type="password"
          value={text}
          autoComplete="off"
          spellCheck={false}
          aria-describedby={`${id}-about`}
          disabled={!available}
          placeholder={!available ? "No secure credential store" : secret.isSet ? "Enter a new value to replace it" : "Paste the value"}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
        <button type="button" className="btn small primary" disabled={text === "" || !available} aria-label={`Save ${secret.label}`} onClick={save}>
          Save
        </button>
        {available && secret.isSet && (
          <button type="button" className="btn small" aria-label={`Remove ${secret.label}`} onClick={() => void change(() => api.plugins.clearSecret(pluginId, secret.key))}>
            Remove
          </button>
        )}
      </div>
    </div>
  );
}

/** What the person calls a legacy secret: the label its plugin gives it, when the plugin is installed. */
function legacyName(overview: PluginsOverview, plugin: string, key: string): string {
  const info = overview.plugins.find((p) => p.plugin.id === plugin);
  return info?.secrets.find((s) => s.key === key)?.label ?? key;
}

const count = (n: number, one: string, many: string): string => `${n} ${n === 1 ? one : many}`;

/**
 * Secrets an earlier version kept in plain text. They are not used. The person
 * moves them into the credential store, or deletes them; nothing happens to
 * them by itself.
 */
export function LegacySecretsNotice({ overview, change }: { overview: PluginsOverview; change: Change }) {
  const legacy: LegacySecrets = overview.legacySecrets;
  const store = overview.secretStore;
  const toast = useStore((s) => s.toast);
  const [confirming, setConfirming] = useState(false);
  if (legacy.entries.length === 0 && !legacy.file) return null;

  const move = () =>
    void change(async () => {
      const r = await api.plugins.migrateLegacySecrets();
      const said: string[] = [];
      if (r.moved > 0) said.push(`${count(r.moved, "secret was", "secrets were")} moved into ${store.name}.`);
      if (r.kept > 0) said.push(`${count(r.kept, "secret was", "secrets were")} already there and kept.`);
      if (r.fileRemoved) said.push("The plain file was overwritten and removed.");
      if (r.failed.length > 0) {
        const one = r.failed.length === 1;
        said.push(`${count(r.failed.length, "secret", "secrets")} could not be moved and ${one ? "is" : "are"} still where ${one ? "it was" : "they were"}: ${r.failed[0]?.reason ?? "unknown reason"}.`);
      }
      toast(said.join(" ") || "There was nothing to move.", r.failed.length > 0 ? "error" : "info");
    });
  const discard = () =>
    void change(async () => {
      const n = await api.plugins.discardLegacySecrets();
      toast(`${count(n, "plain secret was", "plain secrets were")} deleted.`);
    }).then(() => setConfirming(false));

  return (
    <section className="panel legacy-secrets" role="region" aria-label="Secrets kept in plain text">
      <h4>Secrets kept in plain text by an earlier version</h4>
      <p>
        An earlier version of LaunchDeck kept {legacy.entries.length === 1 ? "this secret" : "these secrets"} without encryption
        {legacy.file ? (
          <>
            {" "}
            in <code>{legacy.file}</code>
          </>
        ) : (
          " with the plugin's settings"
        )}
        . <b>{legacy.entries.length === 1 ? "It is" : "They are"} not used</b>: a secret is now read from the system's credential store only.
      </p>
      {legacy.unreadable && <p className="hint warn">The file is there, but this version cannot read it, so nothing in it can be moved. It can be deleted.</p>}
      {legacy.entries.length > 0 && (
        <ul className="plain">
          {legacy.entries.map((e) => (
            <li key={`${e.source}/${e.plugin}/${e.key}`}>
              <b>{e.pluginName ?? e.plugin}</b>
              <span>{legacyName(overview, e.plugin, e.key)}</span>
              <span className="hint">
                {e.source === "file" ? "in the plain file" : "in the plugin's settings"}
                {!e.pluginName && ", plugin not installed"}
              </span>
            </li>
          ))}
        </ul>
      )}
      {!store.available && (
        <p className="hint warn">
          {legacy.entries.length === 1 ? "It" : "They"} cannot be moved, because this computer has no secure credential store. {store.reason}
        </p>
      )}
      <div className="row wrap">
        {store.available && legacy.entries.length > 0 && (
          <button type="button" className="btn small primary" onClick={move}>
            Move to {store.name}
          </button>
        )}
        {confirming ? (
          <>
            <span className="hint">Delete {legacy.entries.length === 1 ? "it" : "them"} without moving? This cannot be undone.</span>
            <button type="button" className="btn small danger" onClick={discard}>
              Yes, delete
            </button>
            <button type="button" className="btn small" onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button type="button" className="btn small" onClick={() => setConfirming(true)}>
            Delete without moving
          </button>
        )}
      </div>
      <p className="hint">
        Moving writes each value to the credential store, reads it back, and only then overwrites and removes the plain copy. Overwriting is the most a program
        can do: a backup or the disk itself may still hold an older copy, so replace a token that matters.
      </p>
    </section>
  );
}
