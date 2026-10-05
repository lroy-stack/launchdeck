// @vitest-environment jsdom
// What the Plugins screen says about a plugin that cannot run. Rust decides the
// kind of problem (`examples/plugins-overview.json`, kept current by a Rust
// contract test); the screen says it in one sentence and keeps the host's own
// words and the folder under Details.

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import overviewJson from "../../../examples/plugins-overview.json";
import { api } from "../../lib/ipc";
import { pluginsOverviewSchema, problemKindSchema, type PluginsOverview, type ProblemKind } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { PluginsView } from "./PluginsView";
import { PROBLEM } from "./text";

const fixture = pluginsOverviewSchema.parse(overviewJson);
const BROKEN = "example.broken";

/** The fixture with the failed plugin and the skipped folder both stopped by `kind`. */
const failedWith = (kind: ProblemKind): PluginsOverview => ({
  ...fixture,
  plugins: fixture.plugins.map((p) =>
    p.plugin.id === BROKEN ? { ...p, plugin: { ...p.plugin, external: { ...p.plugin.external!, kind, error: `card detail for ${kind}` } } } : p,
  ),
  issues: fixture.issues.map((i) => ({ ...i, kind, error: `folder detail for ${kind}` })),
});

let off: () => void = () => undefined;

async function open(overview: PluginsOverview) {
  vi.spyOn(api.plugins, "list").mockImplementation(async () => overview);
  off = await useStore.getState().init();
  useStore.setState({ view: "plugins" });
  render(<PluginsView />);
  await screen.findByRole("heading", { name: "Home Assistant" });
}

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  off();
});

const brokenCard = () => screen.getByText(BROKEN).closest("article")!;
const skipped = () => screen.getByText("skipped").closest("li")!;

/** Open the Details disclosure inside `scope`. */
async function openDetails(scope: HTMLElement) {
  await userEvent.setup().click(within(scope).getByText("Details"));
}

describe("a plugin that cannot run", () => {
  it("says what Rust found in one sentence, and the host's words and the folder only under Details", async () => {
    await open(fixture);
    const card = brokenCard();
    expect(within(card).getByText(PROBLEM.invalidModule)).toBeTruthy();
    expect(card.textContent).not.toContain("wasi_snapshot_preview1");
    expect(card.textContent).not.toContain("<data>/plugins/broken");
    await openDetails(card);
    expect(await within(card).findByText(/wasi_snapshot_preview1::fd_write/)).toBeTruthy();
    expect(within(card).getByText("<data>/plugins/broken")).toBeTruthy();
  });

  it("says the same for a folder that was skipped, naming the folder", async () => {
    await open(fixture);
    const item = skipped();
    expect(within(item).getByText(PROBLEM.invalidManifest)).toBeTruthy();
    expect(within(item).getByText("junk")).toBeTruthy();
    expect(item.textContent).not.toContain("schemaVersion");
    await openDetails(item);
    expect(await within(item).findByText(/`schemaVersion` is required/)).toBeTruthy();
    expect(within(item).getByText("<data>/plugins/junk")).toBeTruthy();
  });

  it.each(problemKindSchema.options)("words %s as one sentence and keeps the raw text for Details", async (kind) => {
    await open(failedWith(kind));
    for (const [scope, raw] of [
      [brokenCard(), `card detail for ${kind}`],
      [skipped(), `folder detail for ${kind}`],
    ] as const) {
      expect(within(scope).getByText(PROBLEM[kind])).toBeTruthy();
      expect(within(scope).queryByText(raw)).toBeNull();
      await openDetails(scope);
      expect(await within(scope).findByText(raw)).toBeTruthy();
    }
  });

  it("has a sentence for every kind, each a full sentence", () => {
    for (const kind of problemKindSchema.options) expect(PROBLEM[kind]).toMatch(/^[A-Z].*\.$/);
  });
});
