// Small colour helpers. LaunchDeck stores colours as #rrggbb everywhere.

const HEX = /^#[0-9a-f]{6}$/i;

export const isHex = (value: string): boolean => HEX.test(value);

/** Accepts "#abc", "abc", "AABBCC" and returns "#aabbcc", or null. */
export function normalizeHex(value: string): string | null {
  let v = value.trim().toLowerCase().replace(/^#/, "");
  if (/^[0-9a-f]{3}$/.test(v)) v = v.split("").map((c) => c + c).join("");
  return /^[0-9a-f]{6}$/.test(v) ? `#${v}` : null;
}

export function hexToRgb(hex: string): [number, number, number] {
  const n = normalizeHex(hex) ?? "#000000";
  return [parseInt(n.slice(1, 3), 16), parseInt(n.slice(3, 5), 16), parseInt(n.slice(5, 7), 16)];
}

export const isOff = (hex: string): boolean => hexToRgb(hex).every((c) => c < 3);

export function withAlpha(hex: string, alpha: number): string {
  const [r, g, b] = hexToRgb(hex);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}

export function luma(hex: string): number {
  const [r, g, b] = hexToRgb(hex);
  return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
}

/** Glyph colour that stays readable on top of a lit pad. */
export const inkFor = (hex: string): string => (luma(hex) > 0.6 ? "rgba(8,8,12,0.85)" : "rgba(255,255,255,0.92)");

export const SWATCHES = [
  "#3b82f6",
  "#22c55e",
  "#a855f7",
  "#ffffff",
  "#ef4444",
  "#f59e0b",
  "#14b8a6",
  "#ec4899",
  "#64748b",
];
