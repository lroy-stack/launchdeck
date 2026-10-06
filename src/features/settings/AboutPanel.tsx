import { Fragment, useState } from "react";
import { ProblemText } from "../../components/Problem";
import type { Problem } from "../../lib/ipc";
import type { AppInfo } from "../../lib/schema";
import { aboutRows, bugReport, releaseOf } from "./text";

const COPIED_FOR_MS = 1500;

/**
 * Settings > About: what Rust's `app_info` says this build is (`info`, or the reason it
 * could not be read in `problem`), and a copy of it for a bug report.
 */
export function AboutPanel({ info, problem }: { info: AppInfo | null; problem: Problem | null }) {
  const [copied, setCopied] = useState(false);
  // When the clipboard refuses, the same text is shown to be selected by hand.
  const [copyFailed, setCopyFailed] = useState(false);

  const copy = (text: string) => {
    const clipboard = navigator.clipboard as Clipboard | undefined;
    if (!clipboard) return setCopyFailed(true);
    clipboard.writeText(text).then(
      () => {
        setCopied(true);
        setTimeout(() => setCopied(false), COPIED_FOR_MS);
      },
      () => setCopyFailed(true),
    );
  };

  return (
    <section className="panel" aria-labelledby="about-title">
      <div className="panel-head">
        <div>
          <h2 id="about-title">About</h2>
          <p className="hint">Which LaunchDeck this is and what it can read. Add it to a bug report.</p>
        </div>
        <button type="button" className="btn" disabled={!info} onClick={() => info && copy(bugReport(info))}>
          {copied ? "Copied" : "Copy for a bug report"}
        </button>
      </div>
      {problem && <ProblemText problem={problem} />}
      {info && (
        <dl className="facts">
          <dt>LaunchDeck</dt>
          <dd>{releaseOf(info)}</dd>
          {aboutRows(info).map(([label, value]) => (
            <Fragment key={label}>
              <dt>{label}</dt>
              <dd>{value}</dd>
            </Fragment>
          ))}
        </dl>
      )}
      {info && copyFailed && (
        <>
          <p className="field-error">The text could not be copied. Select it here and copy it yourself.</p>
          <pre className="snippet">{bugReport(info)}</pre>
        </>
      )}
    </section>
  );
}
