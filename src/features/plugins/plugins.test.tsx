// @vitest-environment jsdom
// What the Plugins screen says about a plugin that cannot run. Rust decides the
// kind of problem (`examples/plugins-overview.json`, kept current by a Rust
// contract test); the screen says it in one sentence and keeps the host's own
// words and the folder under Details.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import overviewJson from "../../../examples/plugins-overview.json";
import { api } from "../../lib/ipc";
import { pluginsOverviewSchema, problemKindSchema, type ExampleInfo, type PluginsOverview, type ProblemKind } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { PluginsView } from "./PluginsView";
import { EXAMPLES_NOTE, NO_PLUGINS, PROBLEM } from "./text";

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

// The example plugins the app ships. Rust lists them (`plugins_examples`, its
// wire shape pinned by `tests/plugin_install.rs`) and installs one on request;
// here the host's answers are stood in for.

const HELLO: ExampleInfo = {
  id: "example.hello",
  name: "Hello Plugin",
  version: "0.1.0",
  description: "Says hello, tells the deck about it, and shows the smallest complete plugin.",
  installed: false,
  network: false,
};
const COUNTER: ExampleInfo = { ...HELLO, id: "example.counter", name: "Counter Plugin", version: "0.2.0", description: "A counter that remembers its value.", installed: true };
const HOME: ExampleInfo = { ...HELLO, id: "example.home-assistant", name: "Home Assistant", description: "Turn lights and switches on and off.", network: true };

/** Nothing installed, nothing skipped, no plain secrets left. */
const EMPTY: PluginsOverview = { ...fixture, plugins: [], issues: [], legacySecrets: { entries: [], unreadable: false } };
/** The overview once Hello is installed: its card, as Rust describes it. */
const WITH_HELLO: PluginsOverview = { ...EMPTY, plugins: fixture.plugins.filter((p) => p.plugin.id === "example.hello") };

/** Stand in for the host: `list` and `examples` answer what is true now; installing changes it. */
function host(overview: PluginsOverview, examples: ExampleInfo[]) {
  let now = { overview, examples };
  vi.spyOn(api.plugins, "list").mockImplementation(async () => now.overview);
  vi.spyOn(api.plugins, "examples").mockImplementation(async () => now.examples);
  return {
    install: vi.spyOn(api.plugins, "installExample").mockImplementation(async (id) => {
      now = { overview: WITH_HELLO, examples: now.examples.map((e) => (e.id === id ? { ...e, installed: true } : e)) };
      return now.overview;
    }),
    reveal: vi.spyOn(api.plugins, "revealFolder").mockImplementation(async () => undefined),
  };
}

async function openWith(overview: PluginsOverview, examples: ExampleInfo[]) {
  const spies = host(overview, examples);
  off = await useStore.getState().init();
  useStore.setState({ view: "plugins" });
  render(<PluginsView />);
  await screen.findByRole("heading", { name: "Example plugins" });
  return spies;
}

const examplesSection = () => screen.getByRole("heading", { name: "Example plugins" }).closest("section")!;
const exampleRow = (name: string) => within(examplesSection()).getByText(name).closest("li")!;

describe("the example plugins the app ships", () => {
  it("lists each with its name, version and description, and Install or Installed", async () => {
    await openWith(EMPTY, [COUNTER, HELLO, HOME]);
    const section = examplesSection();
    expect(within(section).getByText(EXAMPLES_NOTE)).toBeTruthy();
    for (const e of [COUNTER, HELLO, HOME]) {
      const row = exampleRow(e.name);
      expect(within(row).getByText(`v${e.version}`)).toBeTruthy();
      expect(within(row).getByText(e.description)).toBeTruthy();
    }
    expect(within(exampleRow(COUNTER.name)).getByText("Installed")).toBeTruthy();
    expect(within(exampleRow(COUNTER.name)).queryByRole("button")).toBeNull();
    expect(within(exampleRow(HELLO.name)).getByRole("button", { name: "Install Hello Plugin" })).toBeTruthy();
    // The one that asks for the network says so; the others do not.
    expect(exampleRow(HOME.name).textContent).toMatch(/network/i);
    expect(exampleRow(HELLO.name).textContent).not.toMatch(/network/i);
  });

  it("installs on Install, and the plugin's card appears", async () => {
    const { install } = await openWith(EMPTY, [COUNTER, HELLO, HOME]);
    expect(screen.queryByText("example.hello")).toBeNull();
    await userEvent.setup().click(screen.getByRole("button", { name: "Install Hello Plugin" }));
    expect(install).toHaveBeenCalledWith("example.hello");
    const card = (await screen.findByText("example.hello")).closest("article")!;
    expect(within(card).getByRole("heading", { name: "Hello Plugin" })).toBeTruthy();
    await waitFor(() => expect(within(exampleRow(HELLO.name)).getByText("Installed")).toBeTruthy());
    expect(within(exampleRow(HOME.name)).getByRole("button", { name: "Install Home Assistant" })).toBeTruthy();
  });

  it("is not shown when the app ships none", async () => {
    host(EMPTY, []);
    off = await useStore.getState().init();
    render(<PluginsView />);
    await screen.findByText(NO_PLUGINS);
    expect(screen.queryByRole("heading", { name: "Example plugins" })).toBeNull();
  });
});

describe("the Plugins screen with nothing installed", () => {
  it("says so in a sentence, points to the examples and offers the plugins folder and Rescan", async () => {
    const { reveal } = await openWith(EMPTY, [HELLO]);
    const panel = screen.getByRole("heading", { name: "Installed plugins" }).closest("section")!;
    expect(within(panel).getByText(NO_PLUGINS)).toBeTruthy();
    expect(panel.textContent).toMatch(/install an example/i);
    expect(within(panel).getByRole("button", { name: "Rescan" })).toBeTruthy();
    await userEvent.setup().click(within(panel).getByRole("button", { name: "Show plugins folder" }));
    expect(reveal).toHaveBeenCalledTimes(1);
  });

  it("shows no repository path anywhere", async () => {
    await openWith(EMPTY, [HELLO, HOME]);
    await screen.findByText(NO_PLUGINS);
    expect(document.body.textContent).not.toContain("docs/");
  });
});
