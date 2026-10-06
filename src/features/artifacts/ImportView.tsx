// Import a Pack or a Surface file. Rust reads the file and plans what the
// import would do (`artifacts_inspect`); this view shows the plan, sends every
// choice the person makes back whole (`artifacts_preview`) and shows the plan
// Rust returns, then imports with the plan's digest (`artifacts_apply`). It
// computes no id, conflict, dependency or validity itself: Rust does (R9).

import { useCallback, useEffect, useRef, useState } from "react";
import { Disclosure, ProblemText } from "../../components/Problem";
import { api, toProblem, type Problem } from "../../lib/ipc";
import type { ArtifactLimits, ExampleInfo, ImportDecisions, ImportPlan, ImportReport, Rejection } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { PlanSections } from "./PlanSections";
import { EXPIRED, NOTHING_CHANGED, reportText, SHARE_NOTE, STALE, tooLarge } from "./text";
import { TYPING_PAUSE_MS, useAsk } from "./useAsk";

type Stage =
  | { kind: "choose"; note?: Problem }
  | { kind: "reading"; fileName: string }
  | { kind: "rejected"; rejection: Rejection }
  | { kind: "plan"; plan: ImportPlan }
  | { kind: "applied"; report: ImportReport; plan: ImportPlan };

export function ImportView() {
  const attempt = useStore((s) => s.attempt);
  const [stage, setStage] = useState<Stage>({ kind: "choose" });
  const [limits, setLimits] = useState<ArtifactLimits | null>(null);
  const picker = useRef<HTMLInputElement>(null);

  useEffect(() => {
    void attempt(async () => setLimits(await api.artifacts.limits()));
  }, [attempt]);

  const read = async (file: File) => {
    if (limits && file.size > limits.fileBytes) return setStage({ kind: "choose", note: { message: tooLarge(limits.fileBytes) } });
    setStage({ kind: "reading", fileName: file.name });
    try {
      const outcome = await api.artifacts.inspect(new Uint8Array(await file.arrayBuffer()), file.name);
      setStage(outcome.status === "plan" ? { kind: "plan", plan: outcome.plan } : { kind: "rejected", rejection: outcome.rejection });
    } catch (e) {
      setStage({ kind: "choose", note: toProblem(e) });
    }
  };

  const choose = () => picker.current?.click();

  return (
    <div className="page-scroll artifacts">
      <header className="page-head">
        <h1>Import</h1>
        <p>Add the surfaces of a Pack or a Surface file someone shared. You see what it holds and what it would change before anything is added.</p>
      </header>
      <input
        ref={picker}
        type="file"
        hidden
        accept=".ldpack,.ldsurface,application/json"
        aria-label="Pack or Surface file"
        onChange={(e) => {
          const file = e.currentTarget.files?.[0];
          e.currentTarget.value = "";
          if (file) void read(file);
        }}
      />
      {stage.kind === "choose" && <Choose note={stage.note} choose={choose} />}
      {stage.kind === "reading" && <p className="hint">Reading {stage.fileName}…</p>}
      {stage.kind === "rejected" && <Rejected rejection={stage.rejection} choose={choose} />}
      {stage.kind === "plan" && (
        <PlanStage
          key={stage.plan.planId}
          initial={stage.plan}
          limits={limits}
          applied={(report, plan) => setStage({ kind: "applied", report, plan })}
          expired={() => setStage({ kind: "choose", note: { message: EXPIRED } })}
          cancelled={() => setStage({ kind: "choose" })}
        />
      )}
      {stage.kind === "applied" && <Report report={stage.report} plan={stage.plan} again={() => setStage({ kind: "choose" })} />}
    </div>
  );
}

function Choose({ note, choose }: { note?: Problem; choose: () => void }) {
  return (
    <section className="panel">
      {note && <ProblemText problem={note} className="warning" />}
      <p className="hint">{SHARE_NOTE}</p>
      <div className="row">
        <button type="button" className="btn primary" onClick={choose}>
          Choose a file…
        </button>
      </div>
    </section>
  );
}

/** Why Rust refused the file, in its sentence; its code and the place in the file under Details. */
function Rejected({ rejection, choose }: { rejection: Rejection; choose: () => void }) {
  return (
    <section className="panel" role="alert">
      <h2>This file cannot be imported</h2>
      <p className="field-error">{rejection.message}</p>
      <p>{NOTHING_CHANGED}</p>
      <Disclosure>
        <dl className="artifact-details">
          <dt>Code</dt>
          <dd>
            <code>{rejection.code}</code>
          </dd>
          {rejection.at && (
            <>
              <dt>Where</dt>
              <dd>
                <code>{rejection.at}</code>
              </dd>
            </>
          )}
        </dl>
      </Disclosure>
      <div className="row">
        <button type="button" className="btn" onClick={choose}>
          Choose another file…
        </button>
      </div>
    </section>
  );
}

interface PlanStageProps {
  initial: ImportPlan;
  limits: ArtifactLimits | null;
  applied: (report: ImportReport, plan: ImportPlan) => void;
  expired: () => void;
  cancelled: () => void;
}

/** The plan, the person's decisions, and the way back to Rust for each. */
function PlanStage({ initial, limits, applied, expired, cancelled }: PlanStageProps) {
  const attempt = useStore((s) => s.attempt);
  const refreshPlugins = useStore((s) => s.refreshPlugins);
  const { plan, decisions, pending, change, again, replacePlan } = usePlan(initial, expired);
  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<Problem | null>(null);
  const [applying, setApplying] = useState(false);
  const [examples, setExamples] = useState<ExampleInfo[]>([]);

  useEffect(() => {
    void attempt(async () => setExamples(await api.plugins.examples()));
  }, [attempt]);

  const install = async (id: string) => {
    const ok = await attempt(() => api.plugins.installExample(id));
    if (!ok) return;
    await refreshPlugins();
    void attempt(async () => setExamples(await api.plugins.examples()));
    again();
  };

  const confirm = async () => {
    setApplying(true);
    setNotice(null);
    setFailure(null);
    try {
      const outcome = await api.artifacts.apply(plan.planId, decisions, plan.planDigest);
      switch (outcome.status) {
        case "applied":
          return applied(outcome.report, plan);
        case "stale":
          setNotice(STALE);
          return replacePlan(outcome.plan);
        case "blocked":
          return replacePlan(outcome.plan);
        case "failed":
          return setFailure({ message: outcome.message, detail: outcome.detail });
        case "expired":
          return expired();
      }
    } catch (e) {
      setFailure(toProblem(e));
    } finally {
      setApplying(false);
    }
  };

  const cancel = async () => {
    await attempt(() => api.artifacts.discard(plan.planId));
    cancelled();
  };

  return (
    <>
      {notice && <p className="warning">{notice}</p>}
      <PlanSections plan={plan} decisions={decisions} change={change} examples={examples} install={(id) => void install(id)} limits={limits} />
      {failure && (
        <div className="import-failure" role="alert">
          <ProblemText problem={failure} />
        </div>
      )}
      <footer className="import-actions">
        {plan.blockers.length > 0 && (
          <ul className="plain blockers" aria-label="Before importing">
            {plan.blockers.map((b) => (
              <li key={`${b.code}/${b.input ?? ""}`}>{b.message}</li>
            ))}
          </ul>
        )}
        <div className="row">
          <button type="button" className="btn" onClick={() => void cancel()}>
            Cancel
          </button>
          <button type="button" className="btn primary" disabled={!plan.ready || pending || applying} onClick={() => void confirm()}>
            Import
          </button>
        </div>
      </footer>
    </>
  );
}

/**
 * The plan as Rust last made it, and the decisions the person has made. Every
 * change sends all the decisions; a typed one waits for a pause. Only the
 * answer to the latest request is shown.
 */
function usePlan(initial: ImportPlan, expired: () => void) {
  const [plan, setPlan] = useState(initial);
  const [decisions, setDecisions] = useState<ImportDecisions>(initial.decisions);
  const planId = initial.planId;
  const { send, drop, pending } = useAsk(
    (next: ImportDecisions) => api.artifacts.preview(planId, next),
    (outcome) => (outcome.status === "expired" ? expired() : setPlan(outcome.plan)),
  );

  const change = useCallback(
    (next: ImportDecisions, typed = false) => {
      setDecisions(next);
      send(next, typed ? TYPING_PAUSE_MS : 0);
    },
    [send],
  );

  return {
    plan,
    decisions,
    pending,
    change,
    /** Ask again with the same decisions: something else changed (a plugin was installed). */
    again: () => send(decisions),
    /** A plan apply returned (stale or blocked): show it, and let no older preview replace it. */
    replacePlan: (next: ImportPlan) => {
      drop();
      setPlan(next);
    },
  };
}

/** What the import did, and the way to the first surface it added. */
function Report({ report, plan, again }: { report: ImportReport; plan: ImportPlan; again: () => void }) {
  const attempt = useStore((s) => s.attempt);
  const setView = useStore((s) => s.setView);
  const select = useStore((s) => s.select);
  const first = report.surfacesAdded[0] ?? report.surfacesReplaced[0];
  const name = plan.surfaces.find((s) => s.localId === first)?.name;
  const open = () => {
    if (!first) return;
    select(null);
    setView("surface");
    void attempt(() => api.activateSurface(first));
  };
  return (
    <section className="panel" role="status">
      <h2>Imported {plan.artifact.name}</h2>
      {reportText(report).map((line) => (
        <p key={line}>{line}</p>
      ))}
      <div className="row">
        {first && (
          <button type="button" className="btn primary" onClick={open}>
            {name ? `Open ${name}` : "Open the surface"}
          </button>
        )}
        <button type="button" className="btn" onClick={again}>
          Import another file…
        </button>
      </div>
    </section>
  );
}
