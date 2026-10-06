import { useCallback, useEffect, useState } from "react";
import { ProblemText } from "../../components/Problem";
import { api, toProblem, type Problem } from "../../lib/ipc";
import { formatTime, plural } from "../../lib/format";
import { providerLabel, type AdapterView, type AgentView, type HookSetup } from "../../lib/agents";
import { useStore } from "../../stores/runtime";
import { AccessControl, StatusBadge } from "./parts";

const KIND_LABEL: Record<AdapterView["kind"], string> = {
  simulated: "Simulated",
  hooks: "Hooks",
  bridge: "Bridge",
};

export function Adapters() {
  const agents = useStore((s) => s.agents);
  const hooks = agents.adapters.filter((a) => a.kind === "hooks");

  return (
    <>
      <section aria-label="Adapters">
        <h2 className="section-title">Adapters</h2>
        <p className="hint foot">
          An adapter is the only code that knows about a specific tool. It reports what it can see and which commands it can really carry out; everything else stays
          unavailable instead of being faked. Agents that LaunchDeck did not start begin read-only.
        </p>
        <div className="cards wide">
          {agents.adapters.map((a) => (
            <AdapterCard key={a.id} adapter={a} agents={agents.agents.filter((x) => x.adapterId === a.id)} />
          ))}
        </div>
      </section>

      {hooks.length > 0 && <HookEndpoint hooks={hooks} />}
    </>
  );
}

function AdapterCard({ adapter, agents }: { adapter: AdapterView; agents: AgentView[] }) {
  return (
    <article className="card adapter-card">
      <header>
        <h3>{adapter.name}</h3>
        <span className="pill ghost">{KIND_LABEL[adapter.kind]}</span>
        <span className={`pill ${adapter.state === "active" ? "live" : adapter.state === "error" ? "warn" : ""}`}>{adapter.state}</span>
      </header>
      <p>{adapter.description}</p>
      {adapter.detail && (
        <p className={adapter.state === "error" ? "field-error" : "hint"}>
          {adapter.kind === "bridge" && adapter.state === "active" ? (
            <>
              Listening on <code>{adapter.detail}</code>
            </>
          ) : (
            adapter.detail
          )}
        </p>
      )}

      <h4>What it can see and do</h4>
      <ul className="plain bullets">
        {adapter.limits.map((l) => (
          <li key={l}>{l}</li>
        ))}
      </ul>

      <h4>Agents</h4>
      {agents.length === 0 ? (
        <p className="hint">{adapter.state === "active" ? "None connected." : "Nothing registers while this adapter is off."}</p>
      ) : (
        <ul className="plain agent-access">
          {agents.map((a) => (
            <li key={a.id}>
              <span className={`status-dot ${a.online ? "ok" : ""}`} title={a.online ? "Online" : "Offline"} />
              <b>{a.name}</b>
              <small className="hint">{providerLabel(a)}</small>
              <StatusBadge status={a.status} />
              <AccessControl agent={a} />
            </li>
          ))}
        </ul>
      )}

      {adapter.diagnostics.length > 0 && (
        <details>
          <summary>Diagnostics ({adapter.diagnostics.length})</summary>
          <ul className="feed diagnostics">
            {adapter.diagnostics
              .slice(-12)
              .reverse()
              .map((d, i) => (
                <li key={`${d.ts}-${i}`} className={d.level === "error" ? "tone-bad" : ""}>
                  <time>{formatTime(d.ts)}</time>
                  <span className="feed-text">{d.message}</span>
                  <span className="feed-source">{d.level}</span>
                </li>
              ))}
          </ul>
          <p className="hint">Summaries only. Full details are in the application log.</p>
        </details>
      )}
    </article>
  );
}

const WHOLE = /^\d+$/;

/** The opt-in local endpoint harnesses with hooks talk to. Off until the person turns it on. */
function HookEndpoint({ hooks }: { hooks: AdapterView[] }) {
  const settings = useStore((s) => s.agents.settings);
  const attempt = useStore((s) => s.attempt);
  const [port, setPort] = useState(String(settings.hooksPort));
  const [hold, setHold] = useState(String(settings.hooksHoldSecs));
  const [setup, setSetup] = useState<HookSetup | null>(null);
  const [showSetup, setShowSetup] = useState(false);
  const [refused, setRefused] = useState<Problem | null>(null);

  useEffect(() => setPort(String(settings.hooksPort)), [settings.hooksPort]);
  useEffect(() => setHold(String(settings.hooksHoldSecs)), [settings.hooksHoldSecs]);

  const loadSetup = useCallback(() => {
    void attempt(async () => setSetup(await api.agents.hookSetup()));
  }, [attempt]);
  useEffect(() => {
    if (showSetup && settings.hooksEnabled) loadSetup();
  }, [showSetup, settings.hooksEnabled, settings.hooksPort, loadSetup]);

  // Only whether each field holds a whole number; Rust says which values it takes.
  const portOk = WHOLE.test(port.trim());
  const holdOk = WHOLE.test(hold.trim());
  const portNum = Number(port);
  const holdNum = Number(hold);
  const dirty = portNum !== settings.hooksPort || holdNum !== settings.hooksHoldSecs;
  const apply = async () => {
    setRefused(null);
    try {
      await api.agents.updateSettings({ hooksPort: portNum, hooksHoldSecs: holdNum });
    } catch (e) {
      setRefused(toProblem(e));
    }
  };

  return (
    <section className="panel" aria-label="Hook endpoint">
      <div className="panel-head">
        <div>
          <h2>Hook endpoint</h2>
          <p className="hint">
            Lets tools that support hooks (for example Claude Code) report what they are doing and ask LaunchDeck for a permission decision. It listens on{" "}
            <code>127.0.0.1</code> only and every request needs a token. Off by default.
          </p>
        </div>
        <label className="check">
          <input
            type="checkbox"
            checked={settings.hooksEnabled}
            onChange={(e) => void attempt(() => api.agents.updateSettings({ hooksEnabled: e.target.checked }))}
          />
          Enabled
        </label>
      </div>

      <div className="row wrap">
        <label className="field narrow">
          <span>Port</span>
          <input inputMode="numeric" value={port} onChange={(e) => setPort(e.target.value)} aria-invalid={!portOk || refused !== null} />
        </label>
        <label className="field narrow">
          <span>Hold (seconds)</span>
          <input inputMode="numeric" value={hold} onChange={(e) => setHold(e.target.value)} aria-invalid={!holdOk || refused !== null} />
        </label>
        <button
          type="button"
          className="btn small"
          disabled={!dirty || !portOk || !holdOk}
          onClick={() => void apply()}
        >
          Apply
        </button>
      </div>
      {refused && <ProblemText problem={refused} role="alert" />}
      <p className="hint">
        A permission request is held for the hold time so you can answer from the pads or here. If you do not, or if control is not granted, the tool shows its own
        prompt. LaunchDeck never answers for you.
      </p>

      <div className="field-group">
        <h4>Tools</h4>
        <ul className="plain">
          {hooks.map((h) => (
            <li key={h.id}>
              <label className="check">
                <input
                  type="checkbox"
                  checked={settings.hooksHarnesses[h.id] ?? true}
                  disabled={!settings.hooksEnabled}
                  onChange={(e) => void attempt(() => api.agents.updateSettings({ hooksHarnesses: { ...settings.hooksHarnesses, [h.id]: e.target.checked } }))}
                />
                {h.name}
              </label>
              <span className={`pill ${h.state === "active" ? "live" : ""}`}>{h.state}</span>
              <small className="hint">{plural(h.agentIds.length, "agent")}</small>
            </li>
          ))}
        </ul>
      </div>

      <div className="field-group">
        <div className="field-head">
          <h4>Connect a tool</h4>
          <button type="button" className="link" onClick={() => setShowSetup((v) => !v)} aria-expanded={showSetup}>
            {showSetup ? "Hide" : "Show setup"}
          </button>
        </div>
        {showSetup &&
          (!settings.hooksEnabled ? (
            <p className="hint">Enable the endpoint first. The token is created the first time.</p>
          ) : !setup ? (
            <p className="hint">Loading…</p>
          ) : (
            <Setup setup={setup} onRotate={() => void attempt(async () => { await api.agents.rotateHookToken(); setSetup(await api.agents.hookSetup()); })} />
          ))}
      </div>
    </section>
  );
}

function Setup({ setup, onRotate }: { setup: HookSetup; onRotate: () => void }) {
  return (
    <div className="setup">
      <Snippet title={`1. Make the token available to the tool (${setup.envVar})`} text={setup.shellExport} />
      <Snippet title="2. Claude Code: add to .claude/settings.json or ~/.claude/settings.json" text={setup.claudeCodeSettings} />
      <Snippet title="3. Codex: add to ~/.codex/config.toml (written from the docs, not yet tested live)" text={setup.codexConfig} />
      <p className="hint">
        The token stays in the environment, never in the settings files. Tools begin read-only: allow control per agent above to answer permission requests from
        LaunchDeck.
      </p>
      <div>
        <button type="button" className="btn small" onClick={onRotate}>
          Create a new token
        </button>
      </div>
    </div>
  );
}

function Snippet({ title, text }: { title: string; text: string }) {
  const [copied, setCopied] = useState(false);
  const copy = () => {
    void navigator.clipboard
      ?.writeText(text)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => undefined);
  };
  return (
    <div className="snippet-block">
      <div className="field-head">
        <h4>{title}</h4>
        <button type="button" className="btn small" onClick={copy}>
          {copied ? "Copied" : "Copy"}
        </button>
      </div>
      <pre className="snippet">{text}</pre>
    </div>
  );
}
