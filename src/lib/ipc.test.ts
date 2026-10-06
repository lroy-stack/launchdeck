import { describe, expect, it, vi } from "vitest";
import { api, toProblem } from "./ipc";

// A runtime whose replies fit no schema: what an older or newer window would hear.
vi.mock("./preview", () => ({
  createPreviewTransport: async () => ({
    invoke: async () => ({ appVersion: 3 }),
    listen: async () => () => undefined,
  }),
}));

describe("toProblem", () => {
  it("takes a plain string from Rust as the message", () => {
    expect(toProblem("That surface no longer exists.")).toEqual({ message: "That surface no longer exists." });
  });

  it("keeps a command error's message and detail apart", () => {
    expect(toProblem({ message: "The Launchpad could not be reached.", detail: "port busy" })).toEqual({
      message: "The Launchpad could not be reached.",
      detail: "port busy",
    });
    expect(toProblem({ message: "No." })).toEqual({ message: "No." });
  });

  it("uses an Error's message", () => {
    expect(toProblem(new Error("Browser preview: no runtime."))).toEqual({ message: "Browser preview: no runtime." });
  });

  it("says something went wrong for anything else, with the value under detail", () => {
    expect(toProblem({ code: 7 })).toEqual({ message: "Something went wrong.", detail: '{"code":7}' });
    expect(toProblem(undefined)).toEqual({ message: "Something went wrong.", detail: "undefined" });
    expect(toProblem({ message: 3 })).toEqual({ message: "Something went wrong.", detail: '{"message":3}' });
  });
});

describe("a reply that does not fit its schema", () => {
  it("says the window and the runtime disagree, with Zod's text as the detail", async () => {
    const problem = toProblem(await api.appInfo().catch((e: unknown) => e));
    expect(problem.message).toBe("LaunchDeck's window and runtime disagree about app_info. Restart LaunchDeck.");
    expect(problem.detail).toMatch(/appVersion/);
  });
});
