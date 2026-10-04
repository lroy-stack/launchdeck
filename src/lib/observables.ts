// The Observable Catalog on the UI side.
//
// Rust owns it: definitions, providers and the value and status of every
// state. This module only keeps a copy current by applying what Rust pushes,
// and answers lookups. It never derives a status from events or guesses one:
// a state the catalog does not list reads exactly what Rust documents for it.

import type {
  Availability,
  Instance,
  Observable,
  ObservableCatalog,
  Observed,
  ObservablesDelta,
  ProviderInfo,
  Scalar,
} from "./schema";

/** A delta that changes nothing needs no new state: lets the store skip a render. */
export const isEmptyDelta = (d: ObservablesDelta): boolean => !d.catalog && d.providers.length === 0 && d.observables.length === 0;

/** The definitions changed (a plugin came or went): the whole catalog must be fetched again. */
export const needsCatalog = (d: ObservablesDelta): boolean => d.catalog;

const positions = new WeakMap<readonly Observable[], Map<string, number>>();

function positionOf(list: readonly Observable[], id: string): number {
  let map = positions.get(list);
  if (!map) {
    map = new Map(list.map((o, i) => [o.id, i] as const));
    positions.set(list, map);
  }
  return map.get(id) ?? -1;
}

export function findObservable(catalog: ObservableCatalog, id: string): Observable | undefined {
  const at = positionOf(catalog.observables, id);
  return at < 0 ? undefined : catalog.observables[at];
}

/**
 * Applies one `observables-changed` message to a catalog. Pure: returns a new
 * catalog and leaves the old one alone. A message that asks for the whole
 * catalog (`catalog: true`) is not applied here; fetch it instead. Updates for
 * observables the catalog does not list are ignored: the definition arrives
 * with the next full catalog, never with a delta.
 */
export function applyDelta(catalog: ObservableCatalog, delta: ObservablesDelta): ObservableCatalog {
  if (isEmptyDelta(delta) || delta.catalog) return catalog;

  let providers = catalog.providers;
  if (delta.providers.length > 0) {
    providers = [...providers];
    for (const info of delta.providers) {
      const at = providers.findIndex((p) => p.id === info.id);
      if (at >= 0) providers[at] = info;
      else providers.push(info);
    }
  }

  let observables = catalog.observables;
  if (delta.observables.length > 0) {
    observables = [...observables];
    for (const update of delta.observables) {
      const at = positionOf(catalog.observables, update.id);
      const current = at < 0 ? undefined : observables[at];
      if (!current) continue;
      observables[at] = {
        ...current,
        availability: update.availability,
        instances: update.instances,
        instanceCount: update.instanceCount,
      };
    }
    // The copy has the same order, so the positions found for the old list hold.
    positions.set(observables, positions.get(catalog.observables) ?? new Map());
  }
  return { ...catalog, providers, observables };
}

const sameArgs = (a: Record<string, Scalar>, b: Record<string, Scalar>): boolean => {
  const keys = Object.keys(a);
  return keys.length === Object.keys(b).length && keys.every((k) => k in b && a[k] === b[k]);
};

const unavailable: Observed = { status: "unavailable", value: null, updatedAt: null };
const unknown: Observed = { status: "unknown", value: null, updatedAt: null };

/**
 * What a state reads at one address, from the catalog's own entry.
 *
 * A state without arguments has exactly one instance. A scoped state lists the
 * instances that hold a value; an address it does not list reads `unknown`
 * while the provider is up, as Rust documents. Events have no value.
 */
export function readInstance(entry: Observable, args: Record<string, Scalar> = {}): Observed {
  if (entry.kind !== "state") return unavailable;
  if (entry.availability.status === "unavailable") return unavailable;
  const found = entry.instances.find((i: Instance) => sameArgs(i.args, args));
  if (!found) return unknown;
  return { status: found.status, value: found.value ?? null, updatedAt: found.updatedAt ?? null };
}

/** The sentence Rust gives for why an entry cannot be observed, if it cannot. */
export const unavailableReason = (availability: Availability): string | null =>
  availability.status === "unavailable" ? availability.reason : null;

export function providerOf(catalog: ObservableCatalog, id: string): ProviderInfo | undefined {
  return catalog.providers.find((p) => p.id === id);
}

/** The observables of one provider, in the order Rust lists them. */
export const observablesOf = (catalog: ObservableCatalog, provider: string): Observable[] =>
  catalog.observables.filter((o) => o.provider === provider);
