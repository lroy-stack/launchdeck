// Where an arrow key goes on the pad grid, from the cells alone; the keyboard
// behaviour of the rendered surface is in src/features/surfaces/keyboard.test.tsx.

import { describe, expect, it } from "vitest";
import { neighbour, type Cell } from "./PadGrid";

describe("where an arrow goes when controls span several places", () => {
  const cell = (key: string, col: number, row: number, w = 1, h = 1): Cell => ({ key, pos: { kind: "grid", col, row }, control: null, col, row, w, h, aux: false });
  // A 2x2 control at (2,2), single cells around it.
  const big = cell("big", 2, 2, 2, 2);
  const cells = [cell("a", 1, 2), cell("b", 1, 3), big, cell("c", 4, 2), cell("d", 4, 3), cell("e", 2, 1), cell("f", 3, 4)];

  it("treats the big control as one stop and leaves it from its edge", () => {
    expect(neighbour(cells, cells[0]!, "ArrowRight", 4, 4)).toBe(big);
    expect(neighbour(cells, cells[1]!, "ArrowRight", 4, 4)).toBe(big);
    expect(neighbour(cells, big, "ArrowRight", 4, 4)!.key).toBe("c");
    expect(neighbour(cells, big, "ArrowLeft", 4, 4)!.key).toBe("a");
    expect(neighbour(cells, big, "ArrowUp", 4, 4)!.key).toBe("e");
  });

  it("skips a place that holds nothing and stops at the edge", () => {
    // Below the big control, (2,4) holds nothing: there is no pad further down either.
    expect(neighbour(cells, big, "ArrowDown", 4, 4)).toBeUndefined();
    expect(neighbour(cells, cells[5]!, "ArrowLeft", 4, 4)).toBeUndefined();
  });
});
