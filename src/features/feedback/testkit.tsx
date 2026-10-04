// Helpers the feedback editor's tests share: the real Surface editor on the
// preview transport, with the Observable Catalog and workspace Rust emitted.
// A browser has no runtime, so tests that need Rust's answers (review,
// explanation) say what Rust answered with the same shapes it emits.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, vi } from "vitest";
import fixture from "../../../examples/observables.json";
import { api } from "../../lib/ipc";
import {
  controlExplanationSchema,
  controlSchema,
  feedbackReviewSchema,
  type Control,
  type ControlExplanation,
  type ControlPos,
  type FeedbackReview,
} from "../../lib/schema";
import { activePage, useStore } from "../../stores/runtime";
import { SurfaceView } from "../surfaces/SurfaceView";

export const pad = (col: number, row: number): ControlPos => ({ kind: "grid", col, row });

/** The free pad these tests edit: nothing on it in the development surface. */
export const FREE = pad(7, 7);

export const savedAt = (pos: ControlPos): Control | undefined =>
  activePage(useStore.getState())?.controls.find((c) => JSON.stringify(c.pos) === JSON.stringify(pos));

/** Rust's limits and an empty verdict: what `review_feedback` says about a draft with nothing to report. */
export const clean: FeedbackReview = feedbackReviewSchema.parse(fixture.review.clean);

export const review = (over: Partial<FeedbackReview> = {}): FeedbackReview => feedbackReviewSchema.parse({ ...clean, ...over });

export const explained = (name: "missingLook" | "overlay" | "unknown", over: Partial<ControlExplanation> = {}): ControlExplanation =>
  controlExplanationSchema.parse({ ...fixture.explain[name], ...over });

/** The sections of the Inspector, by the names a person reads. */
export const feedback = () => screen.getByRole("region", { name: "Feedback" });
export const inFeedback = () => within(feedback());
export const rule = (n: number) => inFeedback().getByRole("listitem", { name: `Rule ${n}` });
export const eventRule = (n: number) => inFeedback().getByRole("listitem", { name: `Event rule ${n}` });

export function installEditorHarness(): void {
  let off: () => void = () => undefined;
  beforeEach(async () => {
    off = await useStore.getState().init();
    await api.activateSurface("development");
    await api.setMode("edit");
    await waitFor(() => expect(useStore.getState().state?.mode).toBe("edit"));
    // The preview keeps its workspace between tests: leave the free pad free.
    await api.removeControl("development", "home", "g.7.7");
    useStore.setState({ view: "surface", selected: null, intent: null, libraryOpen: false, libraryQuery: "", libraryCategory: null, dragging: false });
  });
  afterEach(() => {
    cleanup();
    off();
    vi.restoreAllMocks();
  });
}

/** Open the editor on a pad. A pad without a control is a new control. */
export function openPad(pos: ControlPos = FREE): void {
  useStore.getState().select(pos);
  render(<SurfaceView />);
}

/** What the runtime would answer for the draft it is given. Mocked, since a browser has no runtime. */
export function runtimeAnswers(answer: (control: Control) => FeedbackReview, explanation?: (control: Control) => ControlExplanation | Error) {
  const reviewFeedback = vi.spyOn(api, "reviewFeedback").mockImplementation(async (_s, _p, control) => answer(controlSchema.parse(control)));
  const explainControl = vi.spyOn(api, "explainControl").mockImplementation(async (_s, _p, id) => {
    const saved = activePage(useStore.getState())?.controls.find((c) => c.id === id);
    const out = explanation?.(saved ?? controlSchema.parse({ id, pos: FREE, label: "", visual: { color: "#000000", animation: "none" } }));
    if (!out) throw new Error("Browser preview: explanations come from the desktop app's runtime.");
    if (out instanceof Error) throw out;
    return out;
  });
  return { reviewFeedback, explainControl };
}
