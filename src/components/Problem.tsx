// A failure as Rust worded it: the sentence for the person, and what it kept for a bug report behind
// Details. React adds no wording of its own here.

import { useState, type ReactNode } from "react";
import type { Problem } from "../lib/ipc";

/** A Details disclosure whose content is in the page only while it is open. */
export function Disclosure({ children, className }: { children: ReactNode; className?: string }) {
  const [open, setOpen] = useState(false);
  return (
    <details className={className} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>Details</summary>
      {open && children}
    </details>
  );
}

interface ProblemTextProps {
  problem: Problem;
  /** Words before the sentence, such as "Failed:" or a heading in bold. */
  lead?: ReactNode;
  className?: string;
  role?: "status" | "alert";
}

/** The sentence, then its detail behind Details when there is one. */
export function ProblemText({ problem, lead, className = "field-error", role }: ProblemTextProps) {
  return (
    <div className="problem" role={role}>
      <p className={className}>
        {lead}
        {lead ? " " : null}
        {problem.message}
      </p>
      {problem.detail && (
        <Disclosure className="tech-details">
          <p className="hint problem-detail">{problem.detail}</p>
        </Disclosure>
      )}
    </div>
  );
}
