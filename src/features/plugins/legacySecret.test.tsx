// @vitest-environment jsdom
// The "Legacy insecure secret" card. Rust decides which credentials of a
// plugin sit in plain text (`secretMigration`, in `examples/plugins-overview.json`,
// kept current by a Rust contract test); the card names them, offers to move
// them into secure storage or to remove them, and offers nothing else.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import overviewJson from "../../../examples/plugins-overview.json";
import { api } from "../../lib/ipc";
import { pluginsOverviewSchema, type PluginInfo, type PluginsOverview, type SecretStoreStatus } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { LEGACY_BODY, LEGACY_TITLE } from "./LegacySecret";
import { PluginsView } from "./PluginsView";

const fixture = pluginsOverviewSchema.parse(overviewJson);
const HELLO = "example.hello";
const KEYCHAIN: SecretStoreStatus = { kind: "keychain", available: true, name: "Keychain" };
const NO_STORE: SecretStoreStatus = { kind: "unavailable", available: false, name: "Keychain", reason: "The login keychain could not be reached." };

let off: () => void = () => undefined;
let current: PluginsOverview;

const withHello = (base: PluginsOverview, edit: (p: PluginInfo) => PluginInfo): PluginsOverview => ({
  ...base,
  plugins: base.plugins.map((p) => (p.plugin.id === HELLO ? edit(structuredClone(p)) : p)),
});

/** Stand in for the host: `plugins_list` answers with what the test says is true now. */
async function open(overview: PluginsOverview) {
  current = overview;
  vi.spyOn(api.plugins, "list").mockImplementation(async () => current);
  off = await useStore.getState().init();
  useStore.setState({ view: "plugins" });
  render(<PluginsView />);
  await screen.findByRole("heading", { name: "Hello Plugin", level: 3 });
}

const hello = () => screen.getByRole("heading", { name: "Hello Plugin", level: 3 }).closest("article")!;
const legacyCard = () => within(hello()).getByRole("region", { name: `${LEGACY_TITLE}: Hello Plugin` });
const queryLegacyCard = () => within(hello()).queryByRole("region", { name: `${LEGACY_TITLE}: Hello Plugin` });
const button = (name: string) => within(legacyCard()).getByRole("button", { name }) as HTMLButtonElement;

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  off();
});

describe("a credential kept in plain text by an earlier version", () => {
  it("says what it is in the agreed words, names the key, and offers exactly Migrate and Remove", async () => {
    await open({ ...fixture, secretStore: KEYCHAIN });
    const region = legacyCard();
    expect(within(region).getByRole("heading", { name: "Legacy insecure secret" })).toBeTruthy();
    expect(region.textContent).toContain(
      "This plugin stores a credential using an obsolete plaintext format. LaunchDeck will no longer use this value. Move it to secure storage to continue using this plugin.",
    );
    expect(LEGACY_BODY).toMatch(/^This plugin stores a credential/);
    expect(within(region).getByText("token")).toBeTruthy();
    const names = within(region)
      .getAllByRole("button")
      .map((b) => b.textContent);
    expect(names).toEqual(["Migrate to secure storage", "Remove legacy value"]);
    // No way to keep using it as it is, anywhere on the screen.
    expect(document.body.textContent).not.toMatch(/continue insecurely|use it anyway|keep using/i);
    expect(screen.queryAllByRole("button").filter((b) => /insecure/i.test(b.textContent ?? ""))).toEqual([]);
    expect(button("Migrate to secure storage").disabled).toBe(false);
  });

  it("is absent for a plugin with nothing in plain text", async () => {
    await open(withHello({ ...fixture, secretStore: KEYCHAIN }, (p) => ({ ...p, secretMigration: undefined })));
    expect(queryLegacyCard()).toBeNull();
    expect(screen.queryByText(LEGACY_TITLE)).toBeNull();
  });

  it("moves it on request, and then says what the plugin needs to use it", async () => {
    const user = userEvent.setup();
    await open({ ...fixture, secretStore: KEYCHAIN });
    const migrate = vi.spyOn(api.plugins, "migratePluginLegacySecrets").mockImplementation(async (id) => {
      current = withHello(current, (p) => ({ ...p, secretMigration: { migrationRequired: [], needsUpdate: ["token"] } }));
      return current.plugins.find((p) => p.plugin.id === id)!;
    });
    await user.click(button("Migrate to secure storage"));
    expect(migrate).toHaveBeenCalledWith(HELLO);
    await waitFor(() => expect(queryLegacyCard()).toBeNull());
    const note = within(hello()).getByRole("note", { name: "Credentials Hello Plugin cannot use" });
    expect(note.textContent).toContain("token is now in secure storage.");
    expect(note.textContent).toContain("it needs a version that declares this credential under secrets (manifest schema 4)");
  });

  it("says on the card why a migration failed, and keeps the card", async () => {
    const user = userEvent.setup();
    await open({ ...fixture, secretStore: KEYCHAIN });
    const why =
      "Nothing was removed from plugins.json, so the plain value is still where it was: `token` could not be moved (the store did not return what was written).";
    vi.spyOn(api.plugins, "migratePluginLegacySecrets").mockRejectedValue(why);
    await user.click(button("Migrate to secure storage"));
    await waitFor(() => expect(within(legacyCard()).getByRole("alert").textContent).toBe(why));
    expect(button("Migrate to secure storage")).toBeTruthy();
  });

  it("cannot be migrated without a secure credential store, and says why", async () => {
    const user = userEvent.setup();
    await open({ ...fixture, secretStore: NO_STORE });
    const migrate = vi.spyOn(api.plugins, "migratePluginLegacySecrets");
    expect(button("Migrate to secure storage").disabled).toBe(true);
    expect(legacyCard().textContent).toContain("It cannot be moved, because this computer has no secure credential store. The login keychain could not be reached.");
    await user.click(button("Migrate to secure storage"));
    expect(migrate).not.toHaveBeenCalled();
    // Removing stays possible.
    expect(button("Remove legacy value").disabled).toBe(false);
  });

  it("removes the plain value only after a second, explicit answer", async () => {
    const user = userEvent.setup();
    await open({ ...fixture, secretStore: KEYCHAIN });
    const remove = vi.spyOn(api.plugins, "removePluginLegacySecrets").mockImplementation(async (id) => {
      current = withHello(current, (p) => ({ ...p, secretMigration: undefined }));
      return current.plugins.find((p) => p.plugin.id === id)!;
    });
    await user.click(button("Remove legacy value"));
    expect(remove).not.toHaveBeenCalled();
    expect(legacyCard().textContent).toContain("This cannot be undone.");
    await user.click(button("Cancel"));
    expect(remove).not.toHaveBeenCalled();
    await user.click(button("Remove legacy value"));
    await user.click(button("Yes, remove"));
    expect(remove).toHaveBeenCalledWith(HELLO);
    await waitFor(() => expect(queryLegacyCard()).toBeNull());
  });
});
