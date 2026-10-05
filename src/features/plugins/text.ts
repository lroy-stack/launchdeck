// The words a plugin's problem is said with. Rust decides the kind; this only
// says it, the same way on a plugin's card and for a folder that was skipped.

import type { ProblemKind } from "../../lib/schema";

export const PROBLEM: Record<ProblemKind, string> = {
  incompatible: "Made for a different LaunchDeck version.",
  invalidManifest: "Its plugin.json has a mistake.",
  invalidModule: "Its code file cannot be loaded.",
  duplicate: "Another plugin with the same id was found first.",
  initFailed: "It failed while starting.",
  stopped: "It stopped after an error.",
};

/** The sentence for a problem; a plain one if the host named no kind. */
export const problemText = (kind?: ProblemKind): string => (kind ? PROBLEM[kind] : "It cannot run.");

/** The last part of a folder path, which is how a person knows the folder. */
export const folderName = (dir: string): string => dir.split(/[\\/]/).filter(Boolean).pop() ?? dir;
