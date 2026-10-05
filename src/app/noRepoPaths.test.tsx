// @vitest-environment jsdom
// The interface never points a person at a file of this repository: someone who
// installed the app has no `docs/` folder. Each view is rendered with what Rust
// emits (the `examples/*.json` fixtures), its disclosures opened.

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import contextJson from "../../examples/context-status.json";
import examplesJson from "../../examples/plugin-examples.json";
import overviewJson from "../../examples/plugins-overview.json";
import { ContextPanel } from "../features/context/ContextPanel";
import { PluginsView } from "../features/plugins/PluginsView";
import { SettingsView } from "../features/settings/SettingsView";
import { api } from "../lib/ipc";
import { contextStatusSchema, exampleInfoSchema, pluginsOverviewSchema } from "../lib/schema";
import { useStore } from "../stores/runtime";

const overview = pluginsOverviewSchema.parse(overviewJson);
const examples = exampleInfoSchema.array().parse(examplesJson);
const contexts = Object.entries(contextJson).map(([name, c]) => [name, contextStatusSchema.parse(c)] as const);

let off: () => void = () => undefined;

beforeEach(async () => {
  vi.spyOn(api.plugins, "list").mockResolvedValue(overview);
  vi.spyOn(api.plugins, "examples").mockResolvedValue(examples);
  off = await useStore.getState().init();
});

afterEach(() => {
  cleanup();
  off();
  vi.restoreAllMocks();
});

/** Everything the view says, with every Details disclosure open. */
function shownText(): string {
  document.querySelectorAll("details").forEach((d) => {
    d.open = true;
    fireEvent(d, new Event("toggle"));
  });
  return document.body.textContent ?? "";
}

describe("no repository paths in the interface", () => {
  it("Plugins", async () => {
    render(<PluginsView />);
    await screen.findByRole("heading", { name: "Home Assistant" });
    expect(shownText()).not.toContain("docs/");
  });

  it.each(contexts)("Auto Context (%s)", (_, context) => {
    useStore.setState({ context });
    render(<ContextPanel />);
    expect(shownText()).not.toContain("docs/");
  });

  it("Settings", () => {
    useStore.setState({ context: contexts[0]![1] });
    render(<SettingsView />);
    expect(screen.getByRole("heading", { name: "Settings", level: 1 })).toBeTruthy();
    expect(shownText()).not.toContain("docs/");
  });
});
