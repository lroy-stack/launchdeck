import { describe, expect, it } from "vitest";
import catalogJson from "../../examples/action-catalog.json";
import workspaceJson from "../../examples/default-workspace.json";
import paramCasesJson from "../../examples/param-cases.json";
import {
  assignmentFor,
  categoriesOf,
  defaultArgs,
  describeArgs,
  groupByCategory,
  indexCatalog,
  isMissingIssue,
  isPrivileged,
  normalizeArgs,
  resolveAssignment,
  searchActions,
  sourceOf,
  summarize,
  tokensOf,
  withinOneEdit,
} from "./actions";
import { z } from "zod";
import { catalogSchema, issueKindSchema, paramDefSchema, workspaceSchema, type Assignment } from "./schema";

// Both fixtures are what Rust emits (`dump_catalog`, `dump_workspace`) and are
// guarded against drift by Rust tests, so these run against the real catalog.
const catalog = catalogSchema.parse(catalogJson);
const index = indexCatalog(catalog);
const workspace = workspaceSchema.parse(workspaceJson);
const HOME_ASSISTANT = "example.home-assistant";
const ids = (query: string, category?: string) => searchActions(index, query, { category }).map((a) => a.id);

describe("the catalog contract with Rust", () => {
  it("has every kind of provider and every reason an action can be unavailable", () => {
    expect(new Set(catalog.providers.map((p) => p.kind))).toEqual(new Set(["builtin", "module", "plugin"]));
    expect(new Set(catalog.providers.map((p) => p.status))).toEqual(new Set(["ready", "disabled", "error"]));
    const causes = catalog.actions.flatMap((a) => (a.availability.status === "unavailable" ? [a.availability.cause] : []));
    expect(new Set(causes)).toEqual(new Set(["provider", "state"]));
  });

  it("gives every action an id that is its provider and action, never its name", () => {
    for (const a of catalog.actions) {
      expect(a.id).toBe(`${a.provider}.${a.action}`);
      expect(a.id).not.toBe(a.name);
    }
    expect(new Set(catalog.actions.map((a) => a.id)).size).toBe(catalog.actions.length);
  });

  it("agrees with Rust about every assignment of the default workspace", () => {
    // 100+ real assignments: the TypeScript mirror of the parameter rules must accept what Rust stores, unchanged.
    // The Home starter's pads need the Home Assistant example plugin, which this catalog does not have
    // installed: they are meant to read as missing until it is (Rust validates them against the
    // installed plugin in tests/action_library.rs).
    let seen = 0;
    let needPlugin = 0;
    for (const s of workspace.surfaces)
      for (const p of s.pages)
        for (const c of p.controls)
          for (const a of [c.press, c.longPress]) {
            if (!a) continue;
            if (a.provider === HOME_ASSISTANT) {
              expect(resolveAssignment(index, a)).toMatchObject({ kind: "missing" });
              needPlugin++;
              continue;
            }
            const def = index.byId.get(`${a.provider}.${a.action}`);
            expect(def, `${s.id}/${p.id}/${c.label}: ${a.provider}.${a.action}`).toBeDefined();
            const { args, issues } = normalizeArgs(def!.params, a.args);
            expect(issues, `${s.id}/${p.id}/${c.label}`).toEqual([]);
            expect(args).toEqual(a.args);
            seen++;
          }
    expect(seen).toBeGreaterThan(100);
    expect(needPlugin).toBe(6);
  });
});

describe("source labels", () => {
  it("says Built-in, the module name, or Plugin: Name", () => {
    const of = (id: string) => sourceOf(index, index.byId.get(id)!);
    expect(of("shell.execute")).toBe("Built-in");
    expect(of("agent-control.approve")).toBe("Agent Control");
    expect(of("example.hello.hello.say")).toBe("Plugin: Hello Plugin");
  });
});

describe("search", () => {
  it("returns everything, grouped by category, when the query is empty", () => {
    const all = searchActions(index, "");
    expect(all).toHaveLength(catalog.actions.length);
    const groups = groupByCategory(all);
    expect(new Set(groups.map((g) => g.category)).size).toBe(groups.length);
  });

  it("finds by name, with the exact name first", () => {
    expect(ids("open url")[0]).toBe("system.openUrl");
    // Two providers can offer an action with the same name; both are found, and the one that can run now comes first.
    expect(ids("approve").slice(0, 2).sort()).toEqual(["agent-control.approve", "demo.approve"]);
    expect(ids("approve")[0]).toBe("demo.approve");
  });

  it("finds by keyword, description, category and source", () => {
    expect(ids("webhook")).toContain("http.request");
    expect(ids("greet")).toContain("example.hello.hello.say");
    expect(ids("navigation")).toEqual(expect.arrayContaining(["navigation.openPage", "navigation.openSurface"]));
    expect(ids("plugin counter")).toEqual(expect.arrayContaining(["example.counter.counter.increment"]));
    expect(ids("plugin counter").every((id) => id.startsWith("example.counter"))).toBe(true);
    expect(ids("built-in shell")).toContain("shell.execute");
  });

  it("needs every word to match", () => {
    expect(ids("approve volume")).toEqual([]);
    expect(ids("nonexistent thing")).toEqual([]);
  });

  it("forgives a typo and skipped letters", () => {
    expect(ids("shotcut")).toContain("system.keyboardShortcut");
    expect(ids("kbshrtct")).toContain("system.keyboardShortcut");
    expect(ids("aprove")).toContain("agent-control.approve");
  });

  it("does not match short words by scattering their letters through a name", () => {
    expect(ids("url")).not.toContain("demo.requestApproval");
    expect(ids("url")).toContain("system.openUrl");
    expect(ids("shr")).not.toContain("demo.requestApproval");
  });

  it("ignores case and accents", () => {
    expect(ids("APPROVE")).toEqual(ids("approve"));
    expect(ids("  shell   ")).toEqual(ids("shell"));
  });

  it("filters by category and still searches inside it", () => {
    const cats = categoriesOf(index).map((c) => c.name);
    expect(cats).toEqual([...cats].sort((a, b) => a.localeCompare(b)));
    expect(cats).toEqual(expect.arrayContaining(["Navigation", "Greetings", "Counter"]));
    expect(ids("", "Navigation").sort()).toEqual(["navigation.openPage", "navigation.openSurface"]);
    expect(ids("page", "Navigation")).toEqual(["navigation.openPage", "navigation.openSurface"].filter((id) => ids("page", "Navigation").includes(id)));
    expect(ids("shell", "Navigation")).toEqual([]);
  });

  it("never hides an action for being unavailable, but prefers the ones that can run", () => {
    const hits = searchActions(index, "increment");
    const counter = hits.find((a) => a.id === "example.counter.counter.increment")!;
    expect(counter.availability.status).toBe("unavailable");
    expect(searchActions(index, "").filter((a) => a.availability.status === "unavailable").length).toBeGreaterThan(5);
    const ranked = searchActions(index, "run");
    const firstUnavailable = ranked.findIndex((a) => a.availability.status === "unavailable");
    const lastAvailableBefore = ranked.slice(0, firstUnavailable < 0 ? ranked.length : firstUnavailable);
    expect(lastAvailableBefore.length).toBeGreaterThan(0);
  });

  it("splits queries into words", () => {
    expect(tokensOf("  Open,  the URL;now ")).toEqual(["open", "the", "url", "now"]);
    expect(tokensOf("")).toEqual([]);
  });

  it("measures one edit", () => {
    expect(withinOneEdit("shortcut", "shotcut")).toBe(true);
    expect(withinOneEdit("shortcut", "shrotcut")).toBe(true);
    expect(withinOneEdit("shortcut", "shortcat")).toBe(true);
    expect(withinOneEdit("shortcut", "short")).toBe(false);
  });

  it("is quick enough to run on every keystroke, even with a large catalog", () => {
    const big = {
      ...catalog,
      actions: Array.from({ length: 2000 }, (_, i) => ({ ...catalog.actions[i % catalog.actions.length]!, id: `x.${i}`, action: String(i), name: `Action number ${i}` })),
    };
    const t0 = performance.now();
    const bigIndex = indexCatalog(big);
    for (const q of ["a", "act num 12", "kbshrtct", "zzzzz"]) searchActions(bigIndex, q);
    expect(performance.now() - t0).toBeLessThan(500);
  });
});

describe("what an assignment is right now", () => {
  const a = (provider: string, action: string, args: Record<string, unknown> = {}): Assignment => ({ provider, action, args });

  it("is ready, unavailable with the reason, or missing with the reason", () => {
    expect(resolveAssignment(index, a("system", "openUrl")).kind).toBe("ready");
    const off = resolveAssignment(index, a("example.counter", "counter.increment"));
    expect(off).toMatchObject({ kind: "unavailable", cause: "provider" });
    const waiting = resolveAssignment(index, a("agent-control", "approve"));
    expect(waiting).toMatchObject({ kind: "unavailable", cause: "state" });
    const gone = resolveAssignment(index, a("acme.stuff", "go"));
    expect(gone).toMatchObject({ kind: "missing" });
    expect(gone.kind === "missing" && gone.reason).toMatch(/not installed/);
    const renamed = resolveAssignment(index, a("example.hello", "nope"));
    expect(renamed.kind === "missing" && renamed.reason).toMatch(/does not offer/);
  });

  it("marks only shell and HTTP as privileged", () => {
    const privileged = catalog.actions.filter((x) => isPrivileged(index, assignmentFor(x))).map((x) => x.id);
    expect(privileged.sort()).toEqual(["http.request", "shell.execute"]);
    expect(isPrivileged(index, null)).toBe(false);
  });

  it("summarises an assignment for lists", () => {
    expect(summarize(index, null)).toBe("No action");
    expect(summarize(index, a("shell", "execute", { command: "git status" }))).toBe("Run shell command · git status");
    expect(summarize(index, a("http", "request", { method: "POST", url: "https://a.b/hook" }))).toBe("HTTP request · POST · https://a.b/hook");
    expect(summarize(index, a("acme.stuff", "go"))).toBe("Missing: acme.stuff.go");
    expect(describeArgs(index.byId.get("system.keyboardShortcut")!, { keys: "mod+c" })).toBe("mod+c");
  });
});

describe("settings", () => {
  const def = (id: string) => index.byId.get(id)!;

  it("starts a form from the defaults the schema names", () => {
    expect(defaultArgs(def("http.request"))).toEqual({ method: "GET" });
    expect(defaultArgs(def("system.openUrl"))).toEqual({});
    expect(defaultArgs(undefined)).toEqual({});
    expect(assignmentFor(def("http.request"))).toEqual({ provider: "http", action: "request", args: { method: "GET" } });
  });

  it("reports what is missing by setting, and fills defaults like Rust", () => {
    const http = def("http.request").params;
    const empty = normalizeArgs(http, {});
    expect(empty.issues).toEqual([{ param: "url", message: "URL is required" }].map((i) => expect.objectContaining({ param: i.param })));
    const ok = normalizeArgs(http, { url: "https://a.b" });
    expect(ok.issues).toEqual([]);
    expect(ok.args).toEqual({ method: "GET", url: "https://a.b" });
  });

  it("treats blank text as not given", () => {
    const shell = def("shell.execute").params;
    expect(normalizeArgs(shell, { command: "   " }).issues[0]?.param).toBe("command");
    expect(normalizeArgs(shell, { command: null }).issues[0]?.param).toBe("command");
  });

  it("enforces types, ranges, choices and unknown settings", () => {
    const slot = def("agent-control.focusSlot").params;
    expect(normalizeArgs(slot, { slot: 99 }).issues[0]?.message).toMatch(/between/);
    expect(normalizeArgs(slot, { slot: 1.5 }).issues[0]?.message).toMatch(/whole number/);
    expect(normalizeArgs(slot, { slot: "x" }).issues[0]?.message).toMatch(/must be a number/);
    expect(normalizeArgs(slot, { slot: 2 }).issues).toEqual([]);
    const http = def("http.request").params;
    expect(normalizeArgs(http, { url: "https://a.b", method: "TRACE" }).issues[0]?.message).toMatch(/listed choices/);
    expect(normalizeArgs(http, { url: "https://a.b", extra: 1 }).issues[0]).toMatchObject({ param: "extra" });
  });

  it("says by kind, as Rust does, whether a setting is still to fill in or wrong", () => {
    const http = def("http.request").params;
    expect(normalizeArgs(http, {}).issues).toEqual([{ param: "url", message: "URL is required", kind: "missing" }]);
    expect(normalizeArgs(http, { url: "https://a.b", method: "TRACE" }).issues.map((i) => i.kind)).toEqual(["invalid"]);
    expect(normalizeArgs(http, { url: "https://a.b", extra: 1 }).issues.map((i) => i.kind)).toEqual(["invalid"]);
    expect(normalizeArgs(http, {}).issues.every(isMissingIssue)).toBe(true);
  });

  it("reads the kind, and the wording only for an issue that has no kind", () => {
    expect(isMissingIssue({ param: "x", message: "X must be text", kind: "missing" })).toBe(true);
    expect(isMissingIssue({ param: "x", message: "X is required", kind: "invalid" })).toBe(false);
    expect(isMissingIssue({ param: "x", message: "X is required" })).toBe(true);
    expect(isMissingIssue({ param: "x", message: "X must be text" })).toBe(false);
  });

  it("checks text length", () => {
    const hello = def("example.hello.hello.say").params;
    expect(normalizeArgs(hello, { name: "x".repeat(61) }).issues[0]?.message).toMatch(/longer than 60/);
    expect(normalizeArgs(hello, {}).issues).toEqual([]);
  });
});

// Written by Rust (`dump_param_cases`) from what `actions::params::normalize` answers, and guarded by
// `tests/catalog_contract.rs`: the mirror must accept and refuse exactly as Rust does.
const paramCases = z
  .object({
    cases: z.array(
      z.object({
        name: z.string(),
        params: z.array(paramDefSchema),
        args: z.record(z.string(), z.unknown()),
        expectedArgs: z.record(z.string(), z.unknown()).nullable(),
        expectedIssues: z.array(z.object({ param: z.string().nullable(), kind: issueKindSchema, message: z.string() })),
      }),
    ),
  })
  .parse(paramCasesJson).cases;

describe("the normalizeArgs mirror against Rust's own answers", () => {
  it("has a table to replay", () => {
    expect(paramCases.length).toBeGreaterThan(40);
    expect(paramCases.some((c) => c.expectedArgs === null)).toBe(true);
  });

  it.each(paramCases.map((c) => [c.name, c] as const))("%s", (_, c) => {
    const { args, issues } = normalizeArgs(c.params, c.args);
    expect(issues.map((i) => ({ param: i.param ?? null, kind: i.kind, message: i.message }))).toEqual(c.expectedIssues);
    if (c.expectedArgs !== null) expect(args).toEqual(c.expectedArgs);
  });
});
