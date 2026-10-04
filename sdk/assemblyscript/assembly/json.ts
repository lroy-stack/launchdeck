// A small JSON value for plugin code.
//
// AssemblyScript has no built-in JSON, and plugins only need to read a request
// and write a reply, so this is deliberately tiny: a tagged value, a parser
// that returns null instead of throwing, and a serializer. It has no
// dependencies.

const MAX_DEPTH = 64;

export enum Kind {
  Null,
  Bool,
  Number,
  String,
  Array,
  Object,
}

export class Json {
  kind: Kind = Kind.Null;
  private b: bool = false;
  private n: f64 = 0;
  private s: string = "";
  // Array elements, or object values (parallel to `names`).
  private items: Json[] = [];
  private names: string[] = [];

  // ---- constructors -------------------------------------------------------

  static boolean(v: bool): Json {
    const j = new Json();
    j.kind = Kind.Bool;
    j.b = v;
    return j;
  }

  static number(v: f64): Json {
    const j = new Json();
    j.kind = Kind.Number;
    j.n = v;
    return j;
  }

  static string(v: string): Json {
    const j = new Json();
    j.kind = Kind.String;
    j.s = v;
    return j;
  }

  static array(): Json {
    const j = new Json();
    j.kind = Kind.Array;
    return j;
  }

  static object(): Json {
    const j = new Json();
    j.kind = Kind.Object;
    return j;
  }

  // ---- building (all return `this`, so calls chain) -----------------------

  set(name: string, value: Json): Json {
    if (this.kind != Kind.Object) return this;
    const at = this.names.indexOf(name);
    if (at >= 0) {
      this.items[at] = value;
    } else {
      this.names.push(name);
      this.items.push(value);
    }
    return this;
  }

  setString(name: string, v: string): Json {
    return this.set(name, Json.string(v));
  }

  setNumber(name: string, v: f64): Json {
    return this.set(name, Json.number(v));
  }

  setBool(name: string, v: bool): Json {
    return this.set(name, Json.boolean(v));
  }

  push(value: Json): Json {
    if (this.kind == Kind.Array) this.items.push(value);
    return this;
  }

  // ---- reading ------------------------------------------------------------

  get length(): i32 {
    return this.kind == Kind.Array || this.kind == Kind.Object ? this.items.length : 0;
  }

  has(name: string): bool {
    return this.kind == Kind.Object && this.names.indexOf(name) >= 0;
  }

  get(name: string): Json | null {
    if (this.kind != Kind.Object) return null;
    const at = this.names.indexOf(name);
    return at >= 0 ? this.items[at] : null;
  }

  at(index: i32): Json | null {
    if (this.kind != Kind.Array || index < 0 || index >= this.items.length) return null;
    return this.items[index];
  }

  // Typed reads with a default, for the common case.
  getString(name: string, fallback: string = ""): string {
    const v = this.get(name);
    return v != null && v.kind == Kind.String ? v.s : fallback;
  }

  getNumber(name: string, fallback: f64 = 0): f64 {
    const v = this.get(name);
    return v != null && v.kind == Kind.Number ? v.n : fallback;
  }

  getInt(name: string, fallback: i32 = 0): i32 {
    const v = this.get(name);
    return v != null && v.kind == Kind.Number ? <i32>v.n : fallback;
  }

  getBool(name: string, fallback: bool = false): bool {
    const v = this.get(name);
    return v != null && v.kind == Kind.Bool ? v.b : fallback;
  }

  asString(fallback: string = ""): string {
    return this.kind == Kind.String ? this.s : fallback;
  }

  asNumber(fallback: f64 = 0): f64 {
    return this.kind == Kind.Number ? this.n : fallback;
  }

  asBool(fallback: bool = false): bool {
    return this.kind == Kind.Bool ? this.b : fallback;
  }

  // ---- text ---------------------------------------------------------------

  toString(): string {
    const out: string[] = [];
    this.write(out);
    return out.join("");
  }

  private write(out: string[]): void {
    switch (this.kind) {
      case Kind.Null:
        out.push("null");
        break;
      case Kind.Bool:
        out.push(this.b ? "true" : "false");
        break;
      case Kind.Number:
        out.push(numberText(this.n));
        break;
      case Kind.String:
        out.push(quote(this.s));
        break;
      case Kind.Array:
        out.push("[");
        for (let i = 0; i < this.items.length; i++) {
          if (i > 0) out.push(",");
          this.items[i].write(out);
        }
        out.push("]");
        break;
      case Kind.Object:
        out.push("{");
        for (let i = 0; i < this.items.length; i++) {
          if (i > 0) out.push(",");
          out.push(quote(this.names[i]));
          out.push(":");
          this.items[i].write(out);
        }
        out.push("}");
        break;
    }
  }

  // Returns null when `text` is not valid JSON.
  static parse(text: string): Json | null {
    const p = new Parser(text);
    p.skip();
    const v = p.value(0);
    if (p.failed || v == null) return null;
    p.skip();
    if (p.at < text.length) return null;
    return v;
  }
}

function numberText(v: f64): string {
  if (isNaN(v) || !isFinite(v)) return "null"; // JSON has no NaN or Infinity
  // Whole numbers print without ".0" so hosts read them as integers.
  if (v == Math.floor(v) && Math.abs(v) < 9007199254740992.0) return (<i64>v).toString();
  return v.toString();
}

const HEX = "0123456789abcdef";

// One pass to measure, one to write into a string of exactly that size: the
// cost is a few instructions per character, whatever the text holds, so a body
// of several hundred kilobytes fits in a call's CPU budget.
function quote(s: string): string {
  const n = s.length;
  const src = changetype<usize>(s);
  let extra = 0;
  for (let i = 0; i < n; i++) {
    const c = load<u16>(src + (<usize>i << 1));
    if (c == 0x22 || c == 0x5c || c == 0x0a || c == 0x0d || c == 0x09) extra += 1;
    else if (c < 0x20) extra += 5;
  }
  const out = changetype<string>(__new(<usize>(n + extra + 2) << 1, idof<string>()));
  let p = changetype<usize>(out);
  store<u16>(p, 0x22);
  p += 2;
  for (let i = 0; i < n; i++) {
    const c = load<u16>(src + (<usize>i << 1));
    if (c >= 0x20 && c != 0x22 && c != 0x5c) {
      store<u16>(p, c);
      p += 2;
      continue;
    }
    store<u16>(p, 0x5c);
    p += 2;
    if (c == 0x22 || c == 0x5c) {
      store<u16>(p, c);
    } else if (c == 0x0a) {
      store<u16>(p, 0x6e);
    } else if (c == 0x0d) {
      store<u16>(p, 0x72);
    } else if (c == 0x09) {
      store<u16>(p, 0x74);
    } else {
      store<u16>(p, 0x75); // u
      store<u16>(p + 2, 0x30);
      store<u16>(p + 4, 0x30);
      store<u16>(p + 6, <u16>HEX.charCodeAt((c >> 4) & 15));
      store<u16>(p + 8, <u16>HEX.charCodeAt(c & 15));
      p += 8;
    }
    p += 2;
  }
  store<u16>(p, 0x22);
  return out;
}

class Parser {
  at: i32 = 0;
  failed: bool = false;

  constructor(private text: string) {}

  skip(): void {
    while (this.at < this.text.length) {
      const c = this.text.charCodeAt(this.at);
      if (c == 0x20 || c == 0x0a || c == 0x0d || c == 0x09) this.at++;
      else break;
    }
  }

  fail(): Json | null {
    this.failed = true;
    return null;
  }

  value(depth: i32): Json | null {
    if (depth > MAX_DEPTH || this.at >= this.text.length) return this.fail();
    const c = this.text.charCodeAt(this.at);
    if (c == 0x7b) return this.object(depth);
    if (c == 0x5b) return this.array(depth);
    if (c == 0x22) {
      const s = this.str();
      return s == null ? null : Json.string(s);
    }
    if (c == 0x74) return this.word("true", Json.boolean(true));
    if (c == 0x66) return this.word("false", Json.boolean(false));
    if (c == 0x6e) return this.word("null", new Json());
    return this.num();
  }

  word(w: string, v: Json): Json | null {
    if (this.text.substr(this.at, w.length) != w) return this.fail();
    this.at += w.length;
    return v;
  }

  num(): Json | null {
    const start = this.at;
    while (this.at < this.text.length) {
      const c = this.text.charCodeAt(this.at);
      const numeric = (c >= 0x30 && c <= 0x39) || c == 0x2d || c == 0x2b || c == 0x2e || c == 0x65 || c == 0x45;
      if (!numeric) break;
      this.at++;
    }
    if (this.at == start) return this.fail();
    const v = parseFloat(this.text.substring(start, this.at));
    if (isNaN(v)) return this.fail();
    return Json.number(v);
  }

  hex4(): i32 {
    if (this.at + 4 > this.text.length) return -1;
    let v = 0;
    for (let i = 0; i < 4; i++) {
      const c = this.text.charCodeAt(this.at + i);
      let d = -1;
      if (c >= 0x30 && c <= 0x39) d = c - 0x30;
      else if (c >= 0x61 && c <= 0x66) d = c - 0x61 + 10;
      else if (c >= 0x41 && c <= 0x46) d = c - 0x41 + 10;
      if (d < 0) return -1;
      v = (v << 4) | d;
    }
    this.at += 4;
    return v;
  }

  // Finds the closing quote first. A string without escapes (most of them) is
  // one slice; one with escapes is written into a buffer no longer than the
  // text it came from. Either way the cost is linear.
  str(): string | null {
    this.at++; // opening quote
    const text = this.text;
    const src = changetype<usize>(text);
    const start = this.at;
    let end = start;
    let escapes = false;
    while (end < text.length) {
      const c = load<u16>(src + (<usize>end << 1));
      if (c == 0x22) break;
      if (c < 0x20) {
        this.failed = true;
        return null;
      }
      if (c == 0x5c) {
        escapes = true;
        end++; // whatever is escaped is not the closing quote
      }
      end++;
    }
    if (end >= text.length) {
      this.failed = true;
      return null;
    }
    if (!escapes) {
      this.at = end + 1;
      return text.substring(start, end);
    }
    const out = changetype<string>(__new(<usize>(end - start) << 1, idof<string>()));
    let p = changetype<usize>(out);
    let i = start;
    while (i < end) {
      const c = load<u16>(src + (<usize>i << 1));
      i++;
      if (c != 0x5c) {
        store<u16>(p, c);
        p += 2;
        continue;
      }
      const e = load<u16>(src + (<usize>i << 1));
      i++;
      let v: u16 = 0;
      if (e == 0x6e) v = 0x0a;
      else if (e == 0x74) v = 0x09;
      else if (e == 0x72) v = 0x0d;
      else if (e == 0x62) v = 0x08;
      else if (e == 0x66) v = 0x0c;
      else if (e == 0x22 || e == 0x5c || e == 0x2f) v = e;
      else if (e == 0x75) {
        this.at = i;
        const u = this.hex4();
        if (u < 0 || this.at > end) {
          this.failed = true;
          return null;
        }
        i = this.at;
        v = <u16>u;
      } else {
        this.failed = true;
        return null;
      }
      store<u16>(p, v);
      p += 2;
    }
    this.at = end + 1;
    const used = <i32>((p - changetype<usize>(out)) >> 1);
    return out.substring(0, used);
  }

  array(depth: i32): Json | null {
    this.at++;
    const a = Json.array();
    this.skip();
    if (this.at < this.text.length && this.text.charCodeAt(this.at) == 0x5d) {
      this.at++;
      return a;
    }
    while (true) {
      this.skip();
      const v = this.value(depth + 1);
      if (v == null) return null;
      a.push(v);
      this.skip();
      if (this.at >= this.text.length) return this.fail();
      const c = this.text.charCodeAt(this.at++);
      if (c == 0x5d) return a;
      if (c != 0x2c) return this.fail();
    }
    return this.fail();
  }

  object(depth: i32): Json | null {
    this.at++;
    const o = Json.object();
    this.skip();
    if (this.at < this.text.length && this.text.charCodeAt(this.at) == 0x7d) {
      this.at++;
      return o;
    }
    while (true) {
      this.skip();
      if (this.at >= this.text.length || this.text.charCodeAt(this.at) != 0x22) return this.fail();
      const k = this.str();
      if (k == null) return null;
      this.skip();
      if (this.at >= this.text.length || this.text.charCodeAt(this.at++) != 0x3a) return this.fail();
      this.skip();
      const v = this.value(depth + 1);
      if (v == null) return null;
      o.set(k, v);
      this.skip();
      if (this.at >= this.text.length) return this.fail();
      const c = this.text.charCodeAt(this.at++);
      if (c == 0x7d) return o;
      if (c != 0x2c) return this.fail();
    }
    return this.fail();
  }
}
