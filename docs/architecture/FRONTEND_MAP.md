# Codebase map: frontend

Who owns what in `src/` (React, TypeScript) and which directory may import which. The Rust side is in [CODEBASE_MAP.md](CODEBASE_MAP.md). The two maps are separate files on purpose: a Rust package and a TypeScript package can run at the same time and never edit the same map. The rules are in [ENGINEERING_RULES.md](ENGINEERING_RULES.md) (R8, R9, R10 and R20 concern the frontend).

The feature-to-feature edges in section 3 are also held, as data, in `scripts/architecture/web.mjs` (added by package A2), which the architecture check reads. The table here and that file list the same edges: change both in one commit. A new directory under `src/features` needs a row here in the same commit (R1).

## 1. Layers

```text
 shared   src/components, src/lib, src/stores
    ▲
 features src/features/<area>
    ▲
 app      src/app
```

Imports go up this picture only: `shared → features → app` means features build on shared code and the app composes features. In detail:

* `src/lib` is plain TypeScript plus the IPC bridge. It imports no React, no feature, no component and no store.
* `src/stores` holds UI state only. It imports `src/lib`, never a feature or a component.
* `src/components` holds widgets that more than one feature uses. It imports `src/lib` and `src/stores`, never a feature.
* A feature imports shared code freely, and another feature only at a composition point listed in section 3.
* `src/app` may import any feature.
* Only `src/lib/ipc.ts` imports `@tauri-apps`, and only `src/lib/ipc.ts` imports `src/lib/preview.ts` (tests aside: `src/lib/preview.test.ts`, `src/lib/plugins.test.ts`).

The architecture check enforces all of these. ESLint (`no-restricted-imports` in `eslint.config.js`) reports the first three as well, so an editor shows them while the import is typed; the feature-to-feature edges are the architecture check's alone.

## 2. Directories

| Directory | Owns | May import |
| --- | --- | --- |
| `src/app` | `App.tsx`: the window shell, the switch between views, the toasts and the fatal screen; `Sidebar.tsx` (navigation, surfaces, the Auto Context control) | everything |
| `src/components` | `PadGrid.tsx` (the pads of a page, drawn by the surface view and the Agents view's Agent Control surface tab); `ActivityFeed.tsx` (recent events, on Home and in the live panel); `forms/ParamForm.tsx` (a form drawn from a parameter schema, an action's or an observable's) and `forms/ColorField.tsx` (a colour as a picker, a hex field and swatches), which the Inspector and the feedback editor share | `src/lib`, `src/stores` |
| `src/features/agents` | the Agents view: `AgentsView`, `Overview`, `Sessions`, `Adapters`, `SurfaceTab` and their `parts` | shared |
| `src/features/context` | Auto Context: `ContextPanel` (rules, fallback, status), `ContextControl` (the sidebar switch), `text.ts` (the wording) | shared |
| `src/features/editor` | the Inspector: `Inspector`, `AssignmentEditor` | shared; `src/features/feedback`, `src/features/library` |
| `src/features/feedback` | the feedback editor: `FeedbackEditor`, `StateRules`, `EventRules`, `LooksEditor`, `LookSelect`, `ObservablePicker`, `LiveFeedback`, `useReview`, `parts`, and the test helper `testkit.tsx` | shared |
| `src/features/home` | `HomeView`: the overview (Agent Control, starter surfaces, devices, recent activity) | shared |
| `src/features/library` | the Action Library panel: `ActionLibrary`, `parts` | shared |
| `src/features/plugins` | the Plugins view: `PluginsView` (installed plugins, the empty state, the example plugins the app ships with Install), `Access` (network grants and secrets), `LegacySecret` (the "Legacy insecure secret" card: Migrate or Remove a credential kept in plain text), `text` (the sentence for each kind of plugin problem, Rust decides the kind; the empty state and the example plugins' wording) | shared |
| `src/features/settings` | `SettingsView`: devices, Auto Context, the external events tester, security text, workspace reset | shared; `src/features/context` |
| `src/features/surfaces` | the surface view: `SurfaceView`, `LivePanel` | shared; `src/features/editor`, `src/features/library` |
| `src/lib` | `ipc.ts` (the only bridge to Rust; `api`, events, every reply parsed with Zod), `schema/` (the Zod schemas, one file per domain: `actions`, `observables`, `feedback`, `workspace`, `context`, `runtime` for devices, runtime state, snapshot and frames, `plugins` for plugins, network access, secrets and the example plugins; `schema/index.ts` re-exports them all, so everything imports `./schema`), `agents.ts` (Agent Control schemas and helpers), `preview.ts` (a browser-only stand-in for the runtime, fed by the contract fixtures), `actions.ts` (catalog index, the `normalizeArgs` mirror), `feedbackEdit.ts` (the editor's pure functions, the `operatorsFor` and `problemsOf` mirrors), `observables.ts`, `assign.ts`, `color.ts` and `format.ts` | nothing outside `src/lib` (and the fixtures in `examples/`) |
| `src/stores` | `runtime.ts`: the Zustand store. It mirrors what Rust pushes (snapshot, catalog, agents, context) and keeps purely visual choices (view, selection, toasts) | `src/lib` |

`src/main.tsx` mounts `App`; `src/styles.css` is the single stylesheet.

## 3. Feature-to-feature edges

Found by reading every import in `src/` (`rg "from \"\.\./" src/features src/components`). Only these edges exist; any other feature-to-feature import fails the check.

Allowed composition points:

| From | To | Where | Why |
| --- | --- | --- | --- |
| `src/app` | every feature | `App.tsx` | the app composes the views |
| `src/features/surfaces` | `src/features/editor` | `SurfaceView.tsx` → `Inspector` | the surface view hosts the Inspector in Edit |
| `src/features/surfaces` | `src/features/library` | `SurfaceView.tsx` → `ActionLibrary` | the surface view hosts the Action Library in Edit |
| `src/features/editor` | `src/features/feedback` | `Inspector.tsx` → `FeedbackEditor`, `useReview` | the Inspector hosts the feedback editor |
| `src/features/editor` | `src/features/library` | `AssignmentEditor.tsx` → `library/parts` | the assignment editor shows a library entry |
| `src/features/settings` | `src/features/context` | `SettingsView.tsx` → `ContextPanel` | Settings hosts the Auto Context panel |
| `src/features/feedback` | `src/features/surfaces` | `testkit.tsx` → `SurfaceView` (tests only) | the feedback tests render the whole surface view |
| `src/features/library` | `src/features/surfaces` | `library.test.tsx` → `SurfaceView` (tests only) | the library tests render the whole surface view |

There are no exceptions: package A4 removed the last ones (the editor ↔ feedback cycle through `ColorField` and `ParamForm`, the features importing `PadGrid` and `ActivityFeed` from `surfaces`, and the sidebar importing `context` from `src/components`). Two of their entries are still in `scripts/architecture/web.mjs`, matching no import, only as examples for the check's own test; they go when that test seeds its own rules.

Tests of a feature may also render `src/app/Sidebar.tsx`: `context/context.test.tsx` and `plugins/access.test.tsx` do, to test the Auto Context switch and the badge on Plugins where a person sees them.
