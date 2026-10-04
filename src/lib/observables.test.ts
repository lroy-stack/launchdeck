import { describe, expect, it } from "vitest";
import fixture from "../../examples/observables.json";
import { applyDelta, findObservable, isEmptyDelta, needsCatalog, observablesOf, providerOf, readInstance, unavailableReason } from "./observables";
import { controlExplanationSchema, observableCatalogSchema, observablesDeltaSchema, snapshotSchema } from "./schema";

const catalog = observableCatalogSchema.parse(fixture.catalog);
const get = (id: string) => findObservable(catalog, id)!;

describe("the Observable Catalog from Rust", () => {
  it("parses everything Rust emits, states and events from every provider in one list", () => {
    expect(catalog.observables.length).toBeGreaterThan(40);
    const providers = new Set(catalog.observables.map((o) => o.provider));
    // The app itself, devices, a built-in provider, Agent Control and external plugins.
    for (const p of ["runtime", "device", "demo", "agent-control", "example.counter", "example.channels"]) expect(providers.has(p), p).toBe(true);
    expect(catalog.observables.some((o) => o.kind === "state")).toBe(true);
    expect(catalog.observables.some((o) => o.kind === "event")).toBe(true);
  });

  it("describes a plugin's enum state with the words a person sees", () => {
    const level = get("example.counter.level");
    expect(level.kind).toBe("state");
    expect(level.providerName).toBe("Counter Plugin");
    expect(level.value?.type).toBe("enum");
    expect(level.value?.values.map((v) => v.value)).toEqual(["zero", "low", "high", "max"]);
    expect(level.params).toEqual([]);
    expect(level.instances).toHaveLength(1);
    expect(level.instances[0]).toMatchObject({ status: "known", value: "low" });
  });

  it("describes a scoped state by its arguments, one instance per address", () => {
    const status = get("example.channels.status");
    expect(status.params.map((p) => p.name)).toEqual(["channel"]);
    expect(status.instanceCount).toBe(2);
    expect(status.instances.map((i) => [i.args.channel, i.value])).toEqual([
      ["alpha", "up"],
      ["beta", "down"],
    ]);
  });

  it("shows the same Agent Control states and events as any other provider", () => {
    const slot = get("agent-control.slot.status");
    expect(slot.params.map((p) => p.name)).toEqual(["slot"]);
    expect(slot.instanceCount).toBeGreaterThan(0);
    const events = observablesOf(catalog, "agent-control").filter((o) => o.kind === "event");
    expect(events.map((e) => e.local)).toEqual(expect.arrayContaining(["session.created", "approval.created", "command.sent"]));
  });

  it("describes an event by the payload fields a rule may test", () => {
    const changed = get("example.counter.counter.changed");
    expect(changed.kind).toBe("event");
    expect(changed.instances).toEqual([]);
    expect(changed.fields.map((f) => [f.name, f.type])).toEqual([
      ["value", "number"],
      ["level", "enum"],
    ]);
    expect(changed.fields[1]!.values.map((v) => v.value)).toEqual(["zero", "low", "high", "max"]);
  });

  it("says why an observable cannot be observed, in Rust's words", () => {
    const off = get("example.hello.hello.completed");
    expect(off.availability.status).toBe("unavailable");
    expect(unavailableReason(off.availability)).toBe("The plugin is turned off in Plugins.");
    expect(providerOf(catalog, "example.hello")?.status).toBe("disabled");
    expect(providerOf(catalog, "example.broken")?.status).toBe("error");
    expect(unavailableReason(get("example.counter.level").availability)).toBeNull();
  });

  it("parses the snapshot's observables field and rejects a malformed catalog", () => {
    expect(snapshotSchema.shape.observables).toBeDefined();
    expect(observableCatalogSchema.safeParse({ providers: [], observables: [{ id: 1 }] }).success).toBe(false);
  });
});

describe("reading a state at an address", () => {
  it("reads the instance an address names, or unknown when the provider has not reported it", () => {
    const status = get("example.channels.status");
    expect(readInstance(status, { channel: "alpha" })).toMatchObject({ status: "known", value: "up" });
    expect(readInstance(status, { channel: "beta" })).toMatchObject({ status: "known", value: "down" });
    expect(readInstance(status, { channel: "zeta" })).toEqual({ status: "unknown", value: null, updatedAt: null });
    // A state without arguments has exactly one instance.
    expect(readInstance(get("example.counter.level"))).toMatchObject({ status: "known", value: "low" });
  });

  it("reads unavailable for an unavailable provider and for an event", () => {
    expect(readInstance(get("example.hello.hello.completed")).status).toBe("unavailable");
    expect(readInstance(get("example.counter.counter.changed")).status).toBe("unavailable");
  });
});

describe("applying what Rust pushes", () => {
  const lifecycle = observablesDeltaSchema.parse(fixture.messages.lifecycle);
  const values = observablesDeltaSchema.parse(fixture.messages.values);
  const full = observablesDeltaSchema.parse(fixture.messages.catalog);

  it("parses the three kinds of message", () => {
    expect(needsCatalog(full)).toBe(true);
    expect(needsCatalog(values)).toBe(false);
    expect(values.observables.map((o) => o.id)).toEqual(["example.channels.status", "example.counter.level"]);
    expect(lifecycle.providers.map((p) => p.id)).toEqual(["example.hello"]);
    expect(isEmptyDelta({ catalog: false, providers: [], observables: [] })).toBe(true);
  });

  it("changes a value and nothing else, leaving the old catalog alone", () => {
    const moved = observablesDeltaSchema.parse({
      observables: [
        {
          id: "example.counter.level",
          availability: { status: "available" },
          instanceCount: 1,
          instances: [{ status: "known", value: "max", updatedAt: 5 }],
        },
      ],
    });
    const next = applyDelta(catalog, moved);
    expect(readInstance(findObservable(next, "example.counter.level")!)).toMatchObject({ value: "max" });
    expect(readInstance(get("example.counter.level"))).toMatchObject({ value: "low" });
    // Definition and the other observables are untouched, and kept as they were.
    expect(findObservable(next, "example.counter.level")!.value).toEqual(get("example.counter.level").value);
    expect(findObservable(next, "example.channels.status")).toBe(get("example.channels.status"));
    expect(next.observables).toHaveLength(catalog.observables.length);
  });

  it("follows a plugin being turned off and on again: unavailable, then unknown, then known", () => {
    const off = observablesDeltaSchema.parse({
      providers: [{ id: "example.counter", name: "Counter Plugin", kind: "plugin", version: "0.2.0", description: "", status: "disabled", reason: "The plugin is turned off in Plugins." }],
      observables: [
        {
          id: "example.counter.level",
          availability: { status: "unavailable", cause: "provider", reason: "The plugin is turned off in Plugins." },
          instanceCount: 0,
          instances: [{ status: "unavailable", value: null, updatedAt: null }],
        },
      ],
    });
    const afterOff = applyDelta(catalog, off);
    expect(providerOf(afterOff, "example.counter")?.status).toBe("disabled");
    expect(readInstance(findObservable(afterOff, "example.counter.level")!).status).toBe("unavailable");

    const back = observablesDeltaSchema.parse({
      providers: [{ id: "example.counter", name: "Counter Plugin", kind: "plugin", version: "0.2.0", description: "", status: "ready" }],
      observables: [{ id: "example.counter.level", availability: { status: "available" }, instanceCount: 1, instances: [{ status: "unknown", value: null, updatedAt: null }] }],
    });
    const afterBack = applyDelta(afterOff, back);
    expect(providerOf(afterBack, "example.counter")?.status).toBe("ready");
    // Re-enabled: running, but nothing reported yet. The old value is not remembered.
    expect(readInstance(findObservable(afterBack, "example.counter.level")!)).toEqual({ status: "unknown", value: null, updatedAt: null });

    const published = observablesDeltaSchema.parse({
      observables: [{ id: "example.counter.level", availability: { status: "available" }, instanceCount: 1, instances: [{ status: "known", value: "high", updatedAt: 9 }] }],
    });
    expect(readInstance(findObservable(applyDelta(afterBack, published), "example.counter.level")!)).toMatchObject({ status: "known", value: "high" });
  });

  it("ignores a change to something the catalog does not list, and a request for the full catalog", () => {
    const stray = observablesDeltaSchema.parse({ observables: [{ id: "nobody.nothing", availability: { status: "available" }, instanceCount: 0 }] });
    expect(applyDelta(catalog, stray).observables).toHaveLength(catalog.observables.length);
    expect(applyDelta(catalog, full)).toBe(catalog);
    expect(applyDelta(catalog, observablesDeltaSchema.parse({}))).toBe(catalog);
  });
});

describe("explain_control from Rust", () => {
  const overlay = controlExplanationSchema.parse(fixture.explain.overlay);
  const missing = controlExplanationSchema.parse(fixture.explain.missingLook);
  const silent = controlExplanationSchema.parse(fixture.explain.unknown);

  it("says what is on screen, which layer put it there and what the state rules would show", () => {
    expect(overlay.layer).toBe("overlay");
    expect(overlay.shown).toBe("blink");
    expect(overlay.overlay).toMatchObject({ look: "blink", source: "event", rule: 1, latched: false });
    expect(overlay.derived).toBe("low");
    expect(overlay.winner).toBe(0);
    const [stateRule, eventRule] = overlay.rules;
    expect(stateRule).toMatchObject({ kind: "state", holds: true, decided: true, valueLabel: "Low" });
    expect(stateRule!.observed).toMatchObject({ status: "known", value: "low" });
    expect(stateRule!.observable).toMatchObject({ id: "example.counter.level", providerName: "Counter Plugin", name: "Counter level" });
    expect(eventRule).toMatchObject({ kind: "event", decided: true, lookExists: true });
  });

  it("reports a rule that decided on a look the control does not have", () => {
    expect(missing.layer).toBe("state");
    expect(missing.diagnostics).toEqual([{ kind: "missingLook", rule: 0, look: "nope" }]);
    expect(missing.rules[0]).toMatchObject({ look: "nope", lookExists: false });
  });

  it("gives a reason for every state that reads unknown or unavailable", () => {
    expect(silent.layer).toBe("rest");
    expect(silent.shown).toBeNull();
    expect(silent.rules.map((r) => r.observed?.status)).toEqual(["unavailable", "unknown", "unavailable"]);
    for (const rule of silent.rules) expect(rule.reason, `rule ${rule.index}`).toBeTruthy();
    expect(silent.rules[0]!.reason).toBe("Nothing named `acme.missing` is installed.");
    expect(silent.rules[0]!.observable?.providerName).toBeUndefined();
  });
});
