import { useMemo, useRef, useState } from "react";
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

interface Cell {
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

/**
 * The surface as a physical object: top row, 8x8 grid, side column and logo.
 * Layout comes from device capabilities, never from a hardcoded 8x8.
 */
export function PadGrid({ page, frame, mode, device }: Props) {
  const select = useStore((s) => s.select);
  const selected = useStore((s) => s.selected);
  const showLabels = useStore((s) => s.showLabels);
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

  const style = {
    gridTemplateColumns: `repeat(${width + (hasSide ? 1 : 0)}, 1fr)`,
    gridTemplateRows: `repeat(${height + rowOffset}, 1fr)`,
    aspectRatio: `${width + (hasSide ? 1 : 0)} / ${height + rowOffset}`,
  } as const;

  return (
    <div className="deck-wrap">
      <div className={`deck mode-${mode}`} style={style} role="group" aria-label="Launchpad surface">
        {cells.map((cell) => (
          <Pad
            key={cell.key}
            cell={cell}
            led={ledAt.get(cell.key)}
            mode={mode}
            showLabels={showLabels}
            selected={
              selectedKey === cell.key ||
              (!!selectedControl && selectedControl.id === cell.control?.id && cell.control !== null)
            }
            onSelect={() => select(cell.pos)}
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
  onSelect,
}: {
  cell: Cell;
  led: { color: string; animation: string } | undefined;
  mode: Mode;
  showLabels: boolean;
  selected: boolean;
  onSelect: () => void;
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

  const release = () => {
    if (!down.current) return;
    down.current = false;
    void api.virtualInput(cell.pos, false).catch(() => undefined);
  };

  const label = control ? control.label : cell.aux ? "Unassigned" : "Empty pad";
  const title = control ? `${control.label}${control.description ? `\n${control.description}` : ""}${problem ? `\nAction unavailable. ${problem}` : ""}` : label;

  return (
    <button
      type="button"
      className={["pad", cell.aux ? "aux" : "", lit ? "lit" : "", anim, selected ? "selected" : "", droppable && dragging ? "drop-ready" : "", droppable && over ? "drop-over" : "", problem ? "flagged" : "", control ? "assigned" : "empty", cell.w > 1 && cell.h > 1 ? "big" : cell.w * cell.h > 1 ? "wide" : ""].join(" ")}
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
        down.current = true;
        void api.virtualInput(cell.pos, true).catch(() => undefined);
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
