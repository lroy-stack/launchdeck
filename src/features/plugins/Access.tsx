// What a plugin asks of the network, what was decided, and its secrets.
//
// Everything shown here is what Rust reported: which destinations the plugin
// asks for, how far each one reaches, what the person decided, whether a secret
// holds a value. The interface decides nothing; it shows the exact destination
// and sends the person's answer.

import { useState } from "react";
import { api } from "../../lib/ipc";
import type { GrantStatus, NetworkHost, PluginInfo, PluginSecret, Reach, SecretStoreKind } from "../../lib/schema";

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

const STORE: Record<SecretStoreKind, string> = {
  keychain: "Kept in your system's credential store.",
  file: "Kept in a private file on this computer, readable by your account only: no system credential store is in use.",
  memory: "Kept in memory only, and forgotten when LaunchDeck closes.",
};

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
        <p className="field-error" role="status">
          <b>Last request did not go through.</b> {sentence(failure.message)} <span className="hint">({failure.code})</span>
        </p>
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
export function Secrets({ info, change }: { info: PluginInfo; change: Change }) {
  if (info.secrets.length === 0) return null;
  return (
    <section className="plugin-access" aria-label={`Secrets of ${info.plugin.name}`}>
      <h4>Secrets</h4>
      <p className="hint">
        A secret is used by LaunchDeck on the plugin's behalf and is never handed to the plugin. {STORE[info.secretStore]}
      </p>
      {info.secrets.map((s) => (
        <SecretField key={`${info.plugin.id}/${s.key}/${s.isSet}`} pluginId={info.plugin.id} secret={s} change={change} />
      ))}
    </section>
  );
}

function SecretField({ pluginId, secret, change }: { pluginId: string; secret: PluginSecret; change: Change }) {
  const [text, setText] = useState("");
  const id = `secret-${pluginId}-${secret.key}`;
  const save = () => {
    if (text === "") return;
    void change(() => api.plugins.setSecret(pluginId, secret.key, text)).then((ok) => ok && setText(""));
  };
  return (
    <div className="field secret">
      <div className="secret-head">
        <label htmlFor={id}>{secret.label}</label>
        <span className={`pill ${secret.isSet ? "live" : "ghost"}`}>{secret.isSet ? "Set" : "Not set"}</span>
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
          placeholder={secret.isSet ? "Enter a new value to replace it" : "Paste the value"}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
        <button type="button" className="btn small primary" disabled={text === ""} aria-label={`Save ${secret.label}`} onClick={save}>
          Save
        </button>
        {secret.isSet && (
          <button type="button" className="btn small" aria-label={`Remove ${secret.label}`} onClick={() => void change(() => api.plugins.clearSecret(pluginId, secret.key))}>
            Remove
          </button>
        )}
      </div>
    </div>
  );
}
