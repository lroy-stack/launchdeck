// @vitest-environment jsdom
// The hook endpoint's port and hold time: the form only asks for whole numbers; Rust decides which
// values it takes and says so in its own sentence.

import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api } from "../../lib/ipc";
import { useStore } from "../../stores/runtime";
import { Adapters } from "./Adapters";

let off: () => void = () => undefined;

async function showAdapters() {
  off = await useStore.getState().init();
  return render(<Adapters />);
}

const endpoint = () => screen.getByRole("region", { name: "Hook endpoint" });

beforeEach(() => vi.restoreAllMocks());
afterEach(() => {
  cleanup();
  off();
});

describe("the hook endpoint's settings", () => {
  it("sends any whole number and shows Rust's sentence when it refuses it", async () => {
    const user = userEvent.setup();
    const update = vi.spyOn(api.agents, "updateSettings").mockRejectedValue({ message: "Use a port from 1024 to 65535." });
    await showAdapters();
    const port = within(endpoint()).getByRole("textbox", { name: "Port" });
    await user.clear(port);
    await user.type(port, "80");
    await user.click(within(endpoint()).getByRole("button", { name: "Apply" }));
    expect(update).toHaveBeenCalledWith(expect.objectContaining({ hooksPort: 80 }));
    expect((await within(endpoint()).findByRole("alert")).textContent).toBe("Use a port from 1024 to 65535.");
  });

  it("does not send what is not a whole number", async () => {
    const user = userEvent.setup();
    await showAdapters();
    const hold = within(endpoint()).getByRole("textbox", { name: "Hold (seconds)" });
    await user.clear(hold);
    await user.type(hold, "2.5");
    expect((within(endpoint()).getByRole("button", { name: "Apply" }) as HTMLButtonElement).disabled).toBe(true);
  });
});
