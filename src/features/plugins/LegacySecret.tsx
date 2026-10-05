// A plugin's credential that an earlier version kept in plain text in
// `plugins.json`. Rust names it in `secretMigration` and never uses the value:
// the person moves it into the credential store or removes it. There is no
// way to keep using it as it is.

import { useState } from "react";
import { api, errorMessage } from "../../lib/ipc";
import type { PluginInfo, SecretStoreStatus } from "../../lib/schema";

type Change = (fn: () => Promise<unknown>) => Promise<boolean>;

interface LegacySecretProps {
  info: PluginInfo;
  store: SecretStoreStatus;
  change: Change;
}

/** The name a person knows a credential by: its label as a secret or a setting, else its key. */
function credentialName(info: PluginInfo, key: string): string {
  return info.secrets.find((s) => s.key === key)?.label ?? info.config.find((c) => c.key === key)?.label ?? key;
}

export const LEGACY_TITLE = "Legacy insecure secret";
export const LEGACY_BODY =
  "This plugin stores a credential using an obsolete plaintext format. LaunchDeck will no longer use this value. Move it to secure storage to continue using this plugin.";

export function LegacySecret({ info, store, change }: LegacySecretProps) {
  const [failure, setFailure] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const migration = info.secretMigration;
  if (!migration) return null;
  const id = info.plugin.id;
  const plain = migration.migrationRequired.map((k) => credentialName(info, k));
  const waiting = migration.needsUpdate.map((k) => credentialName(info, k));

  // A failure stays on the card, in the host's sentence; nothing was changed.
  const migrate = () =>
    void change(async () => {
      try {
        await api.plugins.migratePluginLegacySecrets(id);
        setFailure(null);
      } catch (e) {
        setFailure(errorMessage(e));
      }
    });
  const remove = () =>
    void change(async () => {
      await api.plugins.removePluginLegacySecrets(id);
      setFailure(null);
    }).then(() => setConfirming(false));

  return (
    <>
      {plain.length > 0 && (
        <section className="plugin-access legacy-secrets" role="region" aria-label={`${LEGACY_TITLE}: ${info.plugin.name}`}>
          <h4>{LEGACY_TITLE}</h4>
          <p>{LEGACY_BODY}</p>
          <ul className="plain">
            {plain.map((name) => (
              <li key={name}>
                <code>{name}</code>
              </li>
            ))}
          </ul>
          {!store.available && (
            <p className="hint warn">
              It cannot be moved, because this computer has no secure credential store. {store.reason}
            </p>
          )}
          {failure && (
            <p className="field-error" role="alert">
              {failure}
            </p>
          )}
          <div className="row wrap">
            <button type="button" className="btn small primary" disabled={!store.available} title={store.available ? undefined : store.reason} onClick={migrate}>
              Migrate to secure storage
            </button>
            {confirming ? (
              <>
                <span className="hint">Remove the plain value without moving it? This cannot be undone.</span>
                <button type="button" className="btn small danger" onClick={remove}>
                  Yes, remove
                </button>
                <button type="button" className="btn small" onClick={() => setConfirming(false)}>
                  Cancel
                </button>
              </>
            ) : (
              <button type="button" className="btn small" onClick={() => setConfirming(true)}>
                Remove legacy value
              </button>
            )}
          </div>
        </section>
      )}
      {waiting.length > 0 && (
        <p className="hint warn" role="note" aria-label={`Credentials ${info.plugin.name} cannot use`}>
          {waiting.map((name, i) => (
            <span key={name}>
              {i > 0 && ", "}
              <code>{name}</code>
            </span>
          ))}{" "}
          {waiting.length === 1 ? "is" : "are"} now in secure storage. This version of {info.plugin.name} cannot use {waiting.length === 1 ? "it" : "them"}: it needs a
          version that declares {waiting.length === 1 ? "this credential" : "these credentials"} under <code>secrets</code> (manifest schema 4).
        </p>
      )}
    </>
  );
}
