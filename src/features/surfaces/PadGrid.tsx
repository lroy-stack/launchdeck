import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import { api } from "../../lib/ipc";
import { DRAG_TYPE, problemOf } from "../../lib/assign";
import { inkFor, isOff, withAlpha } from "../../lib/color";
import type { Control, ControlPos, DeviceInfo, Frame, Mode, Page } from "../../lib/schema";
import { posKey } from "../../lib/schema";
import { controlAt, useStore } from "../../stores/runtime";

interface Props {
  page: Page;
  frame: Frame;
  mode: Mode;
  device: DeviceInfo | null;
}

const FALLBACK = { grid: { width: 8, height: 8 }, aux: { top: 8, side: 8, logo: true } };

export interface Cell {
  key: string;
  pos: ControlPos;
  control: Control | null;
  /** CSS grid placement, 1-based, on the padded surface layout. */
  col: number;
  row: number;
  w: number;
  h: number;
  aux: boolean;
}

/** The cell that covers a place of the layout, if any (the logo's corner and unsupported places have none). */
const cellAt = (cells: Cell[], col: number, row: number): Cell | undefined => cells.find((c) => col >= c.col && col < c.col + c.w && row >= c.row && row < c.row + c.h);

/**
 * Where an arrow key goes from a cell: the next cell in that direction, skipping
 * places that hold nothing. A control that spans several places is one stop.
 */
export function neighbour(cells: Cell[], from: Cell, key: string, columns: number, rows: number): Cell | undefined {
  const step = key === "ArrowRight" ? [1, 0] : key === "ArrowLeft" ? [-1, 0] : key === "ArrowDown" ? [0, 1] : key === "ArrowUp" ? [0, -1] : null;
  if (step) {
    let col = step[0]! > 0 ? from.col + from.w : step[0]! < 0 ? from.col - 1 : from.col;
    let row = step[1]! > 0 ? from.row + from.h : step[1]! < 0 ? from.row - 1 : from.row;
    while (col >= 1 && col <= columns && row >= 1 && row <= rows) {
      const found = cellAt(cells, col, row);
      if (found && found !== from) return found;
      col += step[0]!;
      row += step[1]!;
    }
    return undefined;
  }
  if (key === "Home" || key === "End") {
    const inRow = cells.filter((c) => from.row >= c.row && from.row < c.row + c.h).sort((a, b) => a.col - b.col);
    return key === "Home" ? inRow[0] : inRow[inRow.length - 1];
  }
  return undefined;
}

/** Reading order: top to bottom, left to right. */
const inOrder = (a: Cell, b: Cell): number => a.row - b.row || a.col - b.col;

/**
 * The surface as a physical object: top row, 8x8 grid, side column and logo.
 * Layout comes from device capabilities, never from a hardcoded 8x8.
 *
 * For the keyboard the whole surface is one stop: Tab enters it on one pad and
 * leaves it, and the arrow keys (with Home and End) move between pads.
 */
export function PadGrid({ page, frame, mode, device }: Props) {
  const select = useStore((s) => s.select);
  const selected = useStore((s) => s.selected);
  const showLabels = useStore((s) => s.showLabels);
  // Actions in flight on this page, as the runtime reports them: `surface/page/control`.
  const pending = useStore((s) => s.state?.pending);
  const place = useStore((s) => (s.state ? `${s.state.activeSurfaceId}/${s.state.activePageId}/` : ""));
  const acting = useMemo(() => new Set((pending ?? []).flatMap((p) => (p.control?.startsWith(place) ? [p.control.slice(place.length)] : []))), [pending, place]);
  const caps = device?.capabilities ?? FALLBACK;
  const width = caps.grid?.width ?? 0;
  const height = caps.grid?.height ?? 0;
  const topCount = caps.aux.top;
  const sideCount = caps.aux.side;
  const hasTop = topCount > 0;
  const hasSide = sideCount > 0;
  const rowOffset = hasTop ? 1 : 0;

  const ledAt = useMemo(() => {
    const map = new Map<string, { color: string; animation: string }>();
    for (const cell of frame.leds) map.set(posKey(cell.pos), cell.led);
    return map;
  }, [frame]);

  const cells = useMemo(() => {
    const out: Cell[] = [];
    const covered = new Set<string>();
    for (const c of page.controls) {
      if (c.pos.kind !== "grid") continue;
      if (c.pos.col >= width || c.pos.row >= height) continue; // unsupported by this device
      for (let dx = 0; dx < c.span.w; dx++) for (let dy = 0; dy < c.span.h; dy++) covered.add(`${c.pos.col + dx}.${c.pos.row + dy}`);
      out.push({ key: posKey(c.pos), pos: c.pos, control: c, col: c.pos.col + 1, row: c.pos.row + 1 + rowOffset, w: c.span.w, h: c.span.h, aux: false });
    }
    for (let r = 0; r < height; r++)
      for (let col = 0; col < width; col++) {
        if (covered.has(`${col}.${r}`)) continue;
        const pos: ControlPos = { kind: "grid", col, row: r };
        out.push({ key: posKey(pos), pos, control: null, col: col + 1, row: r + 1 + rowOffset, w: 1, h: 1, aux: false });
      }
    const aux = (kind: "top" | "side", count: number) => {
      for (let i = 0; i < count; i++) {
        const pos: ControlPos = { kind, index: i };
        const control = page.controls.find((c) => posKey(c.pos) === posKey(pos)) ?? null;
        out.push({
          key: posKey(pos),
          pos,
          control,
          col: kind === "top" ? i + 1 : width + 1,
          row: kind === "top" ? 1 : i + 1 + rowOffset,
          w: 1,
          h: 1,
          aux: true,
        });
      }
    };
    if (hasTop) aux("top", Math.min(topCount, width || topCount));
    if (hasSide) aux("side", Math.min(sideCount, height || sideCount));
    return out;
  }, [page.controls, width, height, rowOffset, hasTop, hasSide, topCount, sideCount]);

  const selectedControl = controlAt(page, selected);
  const selectedKey = selected ? posKey(selected) : null;
  const isSelected = (cell: Cell): boolean => selectedKey === cell.key || (!!selectedControl && selectedControl.id === cell.control?.id && cell.control !== null);

  // The one pad Tab lands on: the pad the keys were last on, else the selected one, else the first.
  const deck = useRef<HTMLDivElement>(null);
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const stop = useMemo(() => {
    const kept = focusKey ? cells.find((c) => c.key === focusKey) : undefined;
    return (kept ?? cells.find(isSelected) ?? [...cells].sort(inOrder)[0])?.key ?? null;
    // `isSelected` reads `selectedKey` and `selectedControl`.
  }, [cells, focusKey, selectedKey, selectedControl]); // eslint-disable-line react-hooks/exhaustive-deps

  const columns = width + (hasSide ? 1 : 0);
  const rows = height + rowOffset;
  const onArrow = (cell: Cell, e: KeyboardEvent<HTMLButtonElement>) => {
    if (e.altKey || e.metaKey) return;
    let to: Cell | undefined;
    if ((e.key === "Home" || e.key === "End") && e.ctrlKey) {
      const all = [...cells].sort(inOrder);
      to = e.key === "Home" ? all[0] : all[all.length - 1];
    } else if (!e.ctrlKey) {
      to = neighbour(cells, cell, e.key, columns, rows);
    }
    if (!e.key.startsWith("Arrow") && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault(); // an arrow at the edge stays put and does not scroll the page
    if (!to) return;
    setFocusKey(to.key);
    deck.current?.querySelector<HTMLElement>(`[data-cell="${to.key}"]`)?.focus();
  };

  const style = {
    gridTemplateColumns: `repeat(${width + (hasSide ? 1 : 0)}, 1fr)`,
    gridTemplateRows: `repeat(${height + rowOffset}, 1fr)`,
    aspectRatio: `${width + (hasSide ? 1 : 0)} / ${height + rowOffset}`,
  } as const;

  return (
    <div className="deck-wrap">
      <div ref={deck} className={`deck mode-${mode}`} style={style} role="group" aria-label="Launchpad surface" aria-description="Use the arrow keys to move between pads.">
        {cells.map((cell) => (
          <Pad
            key={cell.key}
            cell={cell}
            led={ledAt.get(cell.key)}
            mode={mode}
            showLabels={showLabels}
            selected={isSelected(cell)}
            acting={!!cell.control && acting.has(cell.control.id)}
            tabStop={cell.key === stop}
            onSelect={() => select(cell.pos)}
            onFocus={() => setFocusKey(cell.key)}
            onArrow={(e) => onArrow(cell, e)}
          />
        ))}
        {caps.aux.logo && hasSide && hasTop && (
          <div className="logo-led" style={{ gridColumn: width + 1, gridRow: 1 }} title="Logo LED (output only)">
            <span style={ledAt.get("logo") && !isOff(ledAt.get("logo")!.color) ? { background: ledAt.get("logo")!.color } : undefined} />
          </div>
        )}
      </div>
    </div>
  );
}

function Pad({
  cell,
  led,
  mode,
  showLabels,
  selected,
  acting,
  tabStop,
  onSelect,
  onFocus,
  onArrow,
}: {
  cell: Cell;
  led: { color: string; animation: string } | undefined;
  mode: Mode;
  showLabels: boolean;
  selected: boolean;
  /** The pad's action was invoked and has not finished. */
  acting: boolean;
  /** This is the pad Tab lands on. */
  tabStop: boolean;
  onSelect: () => void;
  onFocus: () => void;
  onArrow: (e: KeyboardEvent<HTMLButtonElement>) => void;
}) {
  const { control } = cell;
  const down = useRef(false);
  const index = useStore((s) => s.catalog);
  const dragging = useStore((s) => s.dragging);
  const requestAssign = useStore((s) => s.requestAssign);
  const [over, setOver] = useState(false);
  const problem = control ? problemOf(index, control) : null;
  const droppable = mode === "edit";
  const lit = !!led && !isOff(led.color);
  const color = led?.color ?? "#000000";
  const anim = lit && led!.animation !== "none" ? `anim-${led!.animation}` : "";

  const press = () => {
    if (down.current) return;
    down.current = true;
    void api.virtualInput(cell.pos, true).catch(() => undefined);
  };
  const release = () => {
    if (!down.current) return;
    down.current = false;
    void api.virtualInput(cell.pos, false).catch(() => undefined);
  };
  const isPressKey = (key: string): boolean => key === "Enter" || key === " ";

  const label = control ? control.label : cell.aux ? "Unassigned" : "Empty pad";
  const title = control ? `${control.label}${control.description ? `\n${control.description}` : ""}${problem ? `\nAction unavailable. ${problem}` : ""}` : label;

  return (
    <button
      type="button"
      className={["pad", cell.aux ? "aux" : "", lit ? "lit" : "", anim, selected ? "selected" : "", acting ? "acting" : "", droppable && dragging ? "drop-ready" : "", droppable && over ? "drop-over" : "", problem ? "flagged" : "", control ? "assigned" : "empty", cell.w > 1 && cell.h > 1 ? "big" : cell.w * cell.h > 1 ? "wide" : ""].join(" ")}
      style={{
        gridColumn: `${cell.col} / span ${cell.w}`,
        gridRow: `${cell.row} / span ${cell.h}`,
        ["--led" as string]: lit ? color : "transparent",
        ["--glow" as string]: lit ? withAlpha(color, 0.55) : "transparent",
        ["--ink" as string]: lit ? inkFor(color) : "var(--text-3)",
      }}
      title={title}
      aria-label={problem ? `${label}. Action unavailable. ${problem}` : label}
      aria-pressed={selected}
      aria-busy={acting || undefined}
      data-cell={cell.key}
      tabIndex={tabStop ? 0 : -1}
      onFocus={onFocus}
      onBlur={release}
      onKeyDown={(e) => {
        // Live: Enter or Space is a finger on the pad, held for as long as the key is. Edit: the button's own click selects.
        if (mode !== "edit" && isPressKey(e.key)) {
          e.preventDefault();
          if (!e.repeat) press();
          return;
        }
        onArrow(e);
      }}
      onKeyUp={(e) => {
        if (mode !== "edit" && isPressKey(e.key)) {
          e.preventDefault();
          release();
        }
      }}
      onDragOver={(e) => {
        if (!droppable || !Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "copy";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        if (!droppable || !Array.from(e.dataTransfer.types).includes(DRAG_TYPE)) return;
        const id = e.dataTransfer.getData(DRAG_TYPE);
        if (!id) return;
        e.preventDefault();
        requestAssign({ definitionId: id, slot: "press", pos: cell.pos });
      }}
      onPointerDown={(e) => {
        if (mode === "edit") return;
        e.currentTarget.setPointerCapture(e.pointerId);
        press();
      }}
      onPointerUp={release}
      onPointerCancel={release}
      onClick={() => {
        if (mode === "edit") onSelect();
      }}
    >
      {problem && (
        <span className="pad-flag" aria-hidden="true">
          !
        </span>
      )}
      {control?.icon && <span className="pad-icon">{control.icon}</span>}
      {control && (showLabels || cell.w * cell.h > 1) && <span className="pad-label">{control.label}</span>}
      {!control && mode === "edit" && !cell.aux && <span className="pad-plus">+</span>}
    </button>
  );
}
