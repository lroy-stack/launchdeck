// @vitest-environment jsdom
// The permission prompt, the grant controls and the secret fields of the
// Plugins screen. The data is what Rust emits (`examples/plugins-overview.json`,
// kept current by a Rust contract test); the host's answers to the person's
// decisions are stood in for, since a browser has no plugin host.

import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import overviewJson from "../../../examples/plugins-overview.json";
import { Sidebar } from "../../app/Sidebar";
import { api } from "../../lib/ipc";
import { pluginsOverviewSchema, type PluginInfo, type PluginsOverview } from "../../lib/schema";
import { useStore } from "../../stores/runtime";
import { PluginsView } from "./PluginsView";

const fixture = pluginsOverviewSchema.parse(overviewJson);
const HA = "example.home-assistant";
const ADDRESS = "http://192.168.1.20:8123";

/** The overview with one plugin changed, as the host would answer after a decision. */
const withPlugin = (base: PluginsOverview, id: string, edit: (p: PluginInfo) => PluginInfo): PluginsOverview => ({
  ...base,
  plugins: base.plugins.map((p) => (p.plugin.id === id ? edit(structuredClone(p)) : p)),
});

/** Home Assistant asking for its address, with nothing decided yet. */
const asking = (): PluginsOverview =>
  withPlugin(fixture, HA, (p) => {
    p.network!.status = "needsApproval";
    p.network!.hosts[0]!.status = "needsApproval";
    delete p.network!.hosts[0]!.decidedAt;
    p.network!.stale = [];
    p.secrets[0]!.isSet = false;
    return p;
  });

let off: () => void = () => undefined;
let current: PluginsOverview;

/** Stand in for the host: every command answers, and `plugins_list` returns what the test says is true now. */
function host(overview: PluginsOverview) {
  current = overview;
  const info = (id: string) => current.plugins.find((p) => p.plugin.id === id)!;
  vi.spyOn(api.plugins, "list").mockImplementation(async () => current);
  return {
    setPermission: vi.spyOn(api.plugins, "setPermission").mockImplementation(async (id, origin, decision) => {
      current = withPlugin(current, id, (p) => {
        const h = p.network!.hosts.find((x) => x.origin === origin);
        if (h) h.status = decision ?? "needsApproval";
        p.network!.stale = p.network!.stale.filter((s) => s.origin !== origin);
        p.network!.status = p.network!.hosts.some((x) => x.status === "needsApproval") ? "needsApproval" : p.network!.hosts.some((x) => x.status === "denied") ? "denied" : "granted";
        return p;
      });
      return info(id);
    }),
    setSecret: vi.spyOn(api.plugins, "setSecret").mockImplementation(async (id, key) => {
      current = withPlugin(current, id, (p) => {
        p.secrets.find((s) => s.key === key)!.isSet = true;
        return p;
      });
      return info(id);
    }),
    clearSecret: vi.spyOn(api.plugins, "clearSecret").mockImplementation(async (id, key) => {
      current = withPlugin(current, id, (p) => {
        p.secrets.find((s) => s.key === key)!.isSet = false;
        return p;
      });
      return info(id);
    }),
  };
}

async function open(overview: PluginsOverview) {
  const spies = host(overview);
  off = await useStore.getState().init();
  useStore.setState({ view: "plugins" });
  render(<PluginsView />);
  await screen.findByRole("heading", { name: "Home Assistant" });
  return spies;
}

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  cleanup();
  off();
});

const card = (name: string) => screen.getByRole("heading", { name, level: 3 }).closest("article")!;
const network = (name: string) => within(card(name)).getByRole("region", { name: `Network access for ${name}` });

describe("asking for network access", () => {
  it("names the plugin, the exact destination, how far it reaches and why, and grants nothing by itself", async () => {
    const spies = await open(asking());
    const prompt = within(network("Home Assistant")).getByRole("group", { name: "Home Assistant requests network access" });
    expect(prompt.textContent).toContain("Home Assistant requests network access to:");
    expect(within(prompt).getByText(ADDRESS)).toBeTruthy();
    expect(within(prompt).getByText("Your local network")).toBeTruthy();
    expect(prompt.textContent).toContain("Purpose Connect to your Home Assistant instance.");
    expect(within(prompt).getByText(/from the setting “Home Assistant address”/)).toBeTruthy();
    // The status is a word, and nothing was decided by showing the prompt.
    expect(within(network("Home Assistant")).getByText("Needs approval")).toBeTruthy();
    expect(spies.setPermission).not.toHaveBeenCalled();
    // The word "network access" alone never stands in for the destination.
    expect(within(prompt).getByRole("button", { name: `Allow ${ADDRESS}` })).toBeTruthy();
    expect(within(prompt).getByRole("button", { name: `Deny ${ADDRESS}` })).toBeTruthy();
  });

  it("Allow grants exactly that destination and the prompt gives way to Granted with Revoke", async () => {
    const user = userEvent.setup();
    const spies = await open(asking());
    await user.click(within(network("Home Assistant")).getByRole("button", { name: `Allow ${ADDRESS}` }));
    expect(spies.setPermission).toHaveBeenCalledWith(HA, ADDRESS, "granted");
    await waitFor(() => expect(within(network("Home Assistant")).queryByRole("group", { name: /requests network access/ })).toBeNull());
    expect(within(network("Home Assistant")).getAllByText("Granted").length).toBeGreaterThan(0);
    await user.click(within(network("Home Assistant")).getByRole("button", { name: `Revoke ${ADDRESS}` }));
    expect(spies.setPermission).toHaveBeenLastCalledWith(HA, ADDRESS, null);
    await waitFor(() => expect(within(network("Home Assistant")).getByRole("group", { name: /requests network access/ })).toBeTruthy());
  });

  it("Deny is remembered as Denied and can be turned into Allow later", async () => {
    const user = userEvent.setup();
    const spies = await open(asking());
    await user.click(within(network("Home Assistant")).getByRole("button", { name: `Deny ${ADDRESS}` }));
    expect(spies.setPermission).toHaveBeenCalledWith(HA, ADDRESS, "denied");
    await waitFor(() => expect(within(network("Home Assistant")).getAllByText("Denied").length).toBeGreaterThan(0));
    expect(within(network("Home Assistant")).queryByRole("group", { name: /requests network access/ })).toBeNull();
    await user.click(within(network("Home Assistant")).getByRole("button", { name: `Allow ${ADDRESS}` }));
    expect(spies.setPermission).toHaveBeenLastCalledWith(HA, ADDRESS, "granted");
  });

  it("shows a decision about an address the plugin no longer asks for as covering nothing, with Forget", async () => {
    const user = userEvent.setup();
    const spies = await open(fixture);
    const region = network("Home Assistant");
    expect(within(region).getByText(/no longer asks for/)).toBeTruthy();
    expect(within(region).getByText("http://homeassistant.local:8123")).toBeTruthy();
    expect(within(region).getByText("was denied")).toBeTruthy();
    await user.click(within(region).getByRole("button", { name: "Forget http://homeassistant.local:8123" }));
    expect(spies.setPermission).toHaveBeenCalledWith(HA, "http://homeassistant.local:8123", null);
  });

  it("says which setting the address comes from when there is nothing to ask for yet, and why the last request failed", async () => {
    await open(fixture);
    const region = network("Home Assistant (not set up)");
    expect(region.textContent).toContain("Nothing to ask for yet: the address comes from Home Assistant address under Settings.");
    expect(region.textContent).toMatch(/is not a URL/);
    expect(within(region).queryByRole("button", { name: /^Allow/ })).toBeNull();
    const failure = within(region).getByRole("status");
    expect(failure.textContent).toContain("Last request did not go through.");
    expect(failure.textContent).toContain("(invalid_request)");
  });

  it("shows no network section for a plugin that asks for nothing", async () => {
    await open(fixture);
    expect(within(card("Counter Plugin")).queryByRole("region", { name: /Network access/ })).toBeNull();
    expect(within(card("Counter Plugin")).queryByRole("region", { name: /Secrets/ })).toBeNull();
  });
});

describe("the badge on Plugins", () => {
  it("counts the destinations enabled plugins are waiting for, and goes when they are decided", async () => {
    const user = userEvent.setup();
    host(asking());
    off = await useStore.getState().init();
    useStore.setState({ view: "plugins" });
    render(
      <>
        <Sidebar />
        <PluginsView />
      </>,
    );
    await waitFor(() => expect(screen.getByLabelText("1 permission request waiting")).toBeTruthy());
    await user.click(await screen.findByRole("button", { name: `Allow ${ADDRESS}` }));
    await waitFor(() => expect(screen.queryByLabelText(/permission request/)).toBeNull());
  });
});

describe("settings and secrets", () => {
  it("keeps the address under Settings, by the name its manifest gives it", async () => {
    await open(fixture);
    const field = within(card("Home Assistant")).getByText("Home Assistant address").closest("label")!;
    expect((within(field).getByRole("textbox") as HTMLInputElement).value).toBe(ADDRESS);
  });

  it("stores a secret without ever showing it again, and says where it is sent and kept", async () => {
    const user = userEvent.setup();
    const spies = await open(asking());
    const secrets = within(card("Home Assistant")).getByRole("region", { name: "Secrets of Home Assistant" });
    expect(secrets.textContent).toContain("never handed to the plugin");
    expect(secrets.textContent).toContain("Kept in memory only");
    expect(within(secrets).getByText("Not set")).toBeTruthy();
    expect(secrets.textContent).toContain(`Only ever sent to ${ADDRESS}.`);
    const input = within(secrets).getByLabelText("Long-lived access token", { selector: "input" }) as HTMLInputElement;
    expect(input.type).toBe("password");
    expect((within(secrets).getByRole("button", { name: "Save Long-lived access token" }) as HTMLButtonElement).disabled).toBe(true);

    await user.type(input, "tok-1234-secret");
    await user.click(within(secrets).getByRole("button", { name: "Save Long-lived access token" }));
    expect(spies.setSecret).toHaveBeenCalledWith(HA, "token", "tok-1234-secret");
    const after = () => within(card("Home Assistant")).getByRole("region", { name: "Secrets of Home Assistant" });
    await waitFor(() => expect(within(after()).getByText("Set")).toBeTruthy());
    // The field is empty again and the value is nowhere on the page.
    expect((within(after()).getByLabelText("Long-lived access token", { selector: "input" }) as HTMLInputElement).value).toBe("");
    expect(document.body.innerHTML).not.toContain("tok-1234-secret");

    await user.click(within(after()).getByRole("button", { name: "Remove Long-lived access token" }));
    expect(spies.clearSecret).toHaveBeenCalledWith(HA, "token");
    await waitFor(() => expect(within(after()).getByText("Not set")).toBeTruthy());
  });

  it("takes the secret with Enter as well", async () => {
    const user = userEvent.setup();
    const spies = await open(asking());
    const secrets = within(card("Home Assistant")).getByRole("region", { name: "Secrets of Home Assistant" });
    await user.type(within(secrets).getByLabelText("Long-lived access token", { selector: "input" }), "abc{Enter}");
    expect(spies.setSecret).toHaveBeenCalledWith(HA, "token", "abc");
  });

  it("says a secret has nowhere to go while its address is not set", async () => {
    await open(fixture);
    const secrets = within(card("Home Assistant (not set up)")).getByRole("region", { name: /Secrets/ });
    expect(secrets.textContent).toContain("It has no destination until the address above is set.");
  });
});

describe("a computer without a secure credential store", () => {
  const noStore = (): PluginsOverview => ({
    ...asking(),
    secretStore: { kind: "unavailable", available: false, name: "Secret Service keyring", reason: "No Secret Service answered in this session. Install and unlock one, then restart LaunchDeck." },
    legacySecrets: { entries: [], unreadable: false },
  });

  it("says secrets are unavailable and why, and offers no way to type one", async () => {
    const user = userEvent.setup();
    const spies = await open(noStore());
    const secrets = within(card("Home Assistant")).getByRole("region", { name: "Secrets of Home Assistant" });
    expect(within(secrets).getByText("Unavailable")).toBeTruthy();
    const why = within(secrets).getByRole("note", { name: "Why secrets are unavailable" });
    expect(why.textContent).toContain("Secrets cannot be saved on this computer.");
    expect(why.textContent).toContain("No Secret Service answered in this session.");
    // It says what it will not do instead, and what still works.
    expect(why.textContent).toContain("never written to a plain file");
    expect(why.textContent).toContain("Everything else does.");
    // Not "Not set": that would read as something the person could fix by typing.
    expect(within(secrets).queryByText("Not set")).toBeNull();
    const input = within(secrets).getByLabelText("Long-lived access token", { selector: "input" }) as HTMLInputElement;
    expect(input.disabled).toBe(true);
    expect((within(secrets).getByRole("button", { name: "Save Long-lived access token" }) as HTMLButtonElement).disabled).toBe(true);
    await user.type(input, "abc{Enter}");
    expect(spies.setSecret).not.toHaveBeenCalled();
    // The rest of the card is as usable as ever.
    expect(within(network("Home Assistant")).getByRole("button", { name: `Allow ${ADDRESS}` })).toBeTruthy();
  });

  it("names the store in use when there is one", async () => {
    await open({ ...asking(), secretStore: { kind: "keychain", available: true, name: "Keychain" } });
    const secrets = within(card("Home Assistant")).getByRole("region", { name: "Secrets of Home Assistant" });
    expect(secrets.textContent).toContain("Kept in your system's credential store (Keychain).");
    expect(within(secrets).queryByRole("note")).toBeNull();
  });
});

describe("secrets an earlier version kept in plain text", () => {
  const legacy = (store: PluginsOverview["secretStore"]): PluginsOverview => ({
    ...asking(),
    secretStore: store,
    legacySecrets: {
      file: "/home/me/.local/share/dev.launchdeck.app/secrets.json",
      unreadable: false,
      entries: [
        { plugin: HA, pluginName: "Home Assistant", key: "token", source: "file" },
        { plugin: "gone.plugin", key: "apiKey", source: "file" },
      ],
    },
  });
  const KEYCHAIN = { kind: "keychain" as const, available: true, name: "Keychain" };
  const notice = () => screen.getByRole("region", { name: "Secrets kept in plain text" });

  it("is silent on a clean installation", async () => {
    await open({ ...asking(), legacySecrets: { entries: [], unreadable: false } });
    expect(screen.queryByRole("region", { name: "Secrets kept in plain text" })).toBeNull();
  });

  it("lists them by name, says they are not used, and moves them only when asked", async () => {
    const user = userEvent.setup();
    await open(legacy(KEYCHAIN));
    const move = vi.spyOn(api.plugins, "migrateLegacySecrets").mockImplementation(async () => {
      current = { ...current, legacySecrets: { entries: [], unreadable: false } };
      return { moved: 2, kept: 0, failed: [], fileRemoved: true };
    });
    expect(notice().textContent).toContain("They are not used");
    expect(notice().textContent).toContain("/home/me/.local/share/dev.launchdeck.app/secrets.json");
    // By the name the plugin gives it when the plugin is here, by its key when it is not.
    const rows = within(notice()).getAllByRole("listitem").map((li) => li.textContent);
    expect(rows[0]).toContain("Home Assistant");
    expect(rows[0]).toContain("Long-lived access token");
    expect(rows[1]).toContain("gone.plugin");
    expect(rows[1]).toContain("plugin not installed");
    // It is honest about what removing a file can and cannot do.
    expect(notice().textContent).toContain("a backup or the disk itself may still hold an older copy");
    expect(move).not.toHaveBeenCalled();

    await user.click(within(notice()).getByRole("button", { name: "Move to Keychain" }));
    expect(move).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("region", { name: "Secrets kept in plain text" })).toBeNull());
    expect(useStore.getState().toasts.at(-1)?.text).toBe("2 secrets were moved into Keychain. The plain file was overwritten and removed.");
  });

  it("says which ones could not be moved, and that they are still there", async () => {
    const user = userEvent.setup();
    await open(legacy(KEYCHAIN));
    vi.spyOn(api.plugins, "migrateLegacySecrets").mockResolvedValue({
      moved: 1,
      kept: 0,
      failed: [{ plugin: "gone.plugin", key: "apiKey", reason: "the value is longer than 4096 bytes" }],
      fileRemoved: false,
    });
    await user.click(within(notice()).getByRole("button", { name: "Move to Keychain" }));
    await waitFor(() => expect(useStore.getState().toasts.at(-1)?.tone).toBe("error"));
    expect(useStore.getState().toasts.at(-1)?.text).toBe(
      "1 secret was moved into Keychain. 1 secret could not be moved and is still where it was: the value is longer than 4096 bytes.",
    );
    expect(notice()).toBeTruthy();
  });

  it("cannot move them without a credential store, and says so", async () => {
    await open(legacy({ kind: "unavailable", available: false, name: "Secret Service keyring", reason: "No Secret Service answered in this session." }));
    expect(within(notice()).queryByRole("button", { name: /^Move to/ })).toBeNull();
    expect(notice().textContent).toContain("They cannot be moved, because this computer has no secure credential store. No Secret Service answered in this session.");
    expect(notice().textContent).toContain("They are not used");
    expect(within(notice()).getByRole("button", { name: "Delete without moving" })).toBeTruthy();
  });

  it("deletes them only after a second, explicit answer", async () => {
    const user = userEvent.setup();
    await open(legacy(KEYCHAIN));
    const discard = vi.spyOn(api.plugins, "discardLegacySecrets").mockImplementation(async () => {
      current = { ...current, legacySecrets: { entries: [], unreadable: false } };
      return 2;
    });
    await user.click(within(notice()).getByRole("button", { name: "Delete without moving" }));
    expect(discard).not.toHaveBeenCalled();
    expect(notice().textContent).toContain("This cannot be undone.");
    await user.click(within(notice()).getByRole("button", { name: "Cancel" }));
    expect(discard).not.toHaveBeenCalled();
    await user.click(within(notice()).getByRole("button", { name: "Delete without moving" }));
    await user.click(within(notice()).getByRole("button", { name: "Yes, delete" }));
    expect(discard).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByRole("region", { name: "Secrets kept in plain text" })).toBeNull());
    expect(useStore.getState().toasts.at(-1)?.text).toBe("2 plain secrets were deleted.");
  });

  it("offers no field for a secret setting of an older plugin and never says it is handed over", async () => {
    await open(fixture);
    const hello = card("Hello Plugin");
    expect(hello.textContent).toContain("A credential in an obsolete format. LaunchDeck does not keep or use a value for it");
    expect(within(hello).queryByPlaceholderText(/Enter a/)).toBeNull();
    expect(hello.querySelector('input[type="password"]')).toBeNull();
    expect(document.body.textContent).not.toContain("handed to the plugin. It keeps working");
  });
});

