import { describe, expect, it } from "vitest";
import { toProblem } from "./ipc";

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
