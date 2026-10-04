import { useEffect, useState } from "react";
import { SWATCHES, normalizeHex } from "../../lib/color";

/** A colour as a picker, a hex field and a few swatches. */
export function ColorField({ label, value, onChange }: { label: string; value: string; onChange: (hex: string) => void }) {
  const [text, setText] = useState(value);
  // Follow the colour when something else changes it, never while it is being typed:
  // "#ef4" is already a colour (#eeff44), and rewriting the field would turn "#ef4444" into "#eeff44".
  useEffect(() => setText((t) => (normalizeHex(t) === value ? t : value)), [value]);
  return (
    <div className="field">
      <span>{label}</span>
      <div className="color-row">
        <input type="color" aria-label={`${label} picker`} value={value} onChange={(e) => onChange(e.target.value)} />
        <input
          className="mono"
          value={text}
          spellCheck={false}
          onChange={(e) => {
            setText(e.target.value);
            const hex = normalizeHex(e.target.value);
            if (hex) onChange(hex);
          }}
        />
        <div className="swatches">
          {SWATCHES.map((c) => (
            <button key={c} type="button" className={`swatch${c === value ? " on" : ""}`} style={{ background: c }} aria-label={c} onClick={() => onChange(c)} />
          ))}
        </div>
      </div>
    </div>
  );
}
