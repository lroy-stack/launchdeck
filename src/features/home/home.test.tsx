// @vitest-environment jsdom
// The Overview shows what Rust wrote about each surface; React writes no starter text of its own.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useStore } from "../../stores/runtime";
import { HomeView } from "./HomeView";

let off: () => void = () => undefined;

afterEach(() => {
  cleanup();
  off();
});

describe("the Overview", () => {
  it("describes each surface with its own description", async () => {
    off = await useStore.getState().init();
    const surfaces = useStore.getState().workspace!.surfaces;
    render(<HomeView />);
    for (const s of surfaces) {
      const card = screen.getByRole("heading", { level: 3, name: s.name }).closest("article")!;
      expect(card.querySelector("p")?.textContent).toBe(s.description);
    }
  });

  it("says so when a surface has no description", async () => {
    off = await useStore.getState().init();
    const ws = useStore.getState().workspace!;
    useStore.setState({ workspace: { ...ws, surfaces: [{ ...ws.surfaces[0]!, description: "" }] } });
    render(<HomeView />);
    expect(screen.getByText("No description yet.")).toBeTruthy();
  });
});
