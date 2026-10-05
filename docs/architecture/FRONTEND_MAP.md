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

## 2. Directories

| Directory | Owns | May import |
| --- | --- | --- |
| `src/app` | `App.tsx`: the window shell, the switch between views, the toasts and the fatal screen | everything |
| `src/components` | `Sidebar.tsx` (navigation, surfaces, the Auto Context control); `PadGrid.tsx` (the pads of a page, drawn by the surface view and the Agents view's Agent Control surface tab); `ActivityFeed.tsx` (recent events, on Home and in the live panel); `forms/ParamForm.tsx` (a form drawn from a parameter schema, an action's or an observable's) and `forms/ColorField.tsx` (a colour as a picker, a hex field and swatches), which the Inspector and the feedback editor share | `src/lib`, `src/stores` (today also `src/features/context`: an exception, see section 3) |
| `src/features/agents` | the Agents view: `AgentsView`, `Overview`, `Sessions`, `Adapters`, `SurfaceTab` and their `parts` | shared |
| `src/features/context` | Auto Context: `ContextPanel` (rules, fallback, status), `ContextControl` (the sidebar switch), `text.ts` (the wording) | shared |
| `src/features/devices` | `HomeView` (the overview: Agent Control, starter surfaces, devices, recent activity) and `SettingsView` (devices, Auto Context, the external events tester, security text, workspace reset). A4 moves them to `src/features/home` and `src/features/settings` | shared; `src/features/context` |
| `src/features/editor` | the Inspector: `Inspector`, `AssignmentEditor` | shared; `src/features/feedback`, `src/features/library` |
| `src/features/feedback` | the feedback editor: `FeedbackEditor`, `StateRules`, `EventRules`, `LooksEditor`, `LookSelect`, `ObservablePicker`, `LiveFeedback`, `useReview`, `parts`, and the test helper `testkit.tsx` | shared |
| `src/features/library` | the Action Library panel: `ActionLibrary`, `parts` | shared |
| `src/features/plugins` | the Plugins view: `PluginsView`, `Access` (network grants and secrets) | shared |
| `src/features/surfaces` | the surface view: `SurfaceView`, `LivePanel` | shared; `src/features/editor`, `src/features/library` |
| `src/lib` | `ipc.ts` (the only bridge to Rust; `api`, events, every reply parsed with Zod), `schema.ts` (the Zod schemas; A4 splits it into `src/lib/schema/`), `agents.ts` (Agent Control schemas and helpers), `preview.ts` (a browser-only stand-in for the runtime, fed by the contract fixtures), `actions.ts` (catalog index, the `normalizeArgs` mirror), `feedbackEdit.ts` (the editor's pure functions, the `operatorsFor` and `problemsOf` mirrors), `observables.ts`, `assign.ts`, `color.ts`, `format.ts`, and the unused `feedback.ts` (A4 deletes it) | nothing outside `src/lib` (and the fixtures in `examples/`) |
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
| `src/features/devices` | `src/features/context` | `SettingsView.tsx` → `ContextPanel` | Settings hosts the Auto Context panel; becomes `settings` → `context` when A4 moves the view |
| `src/features/feedback` | `src/features/surfaces` | `testkit.tsx` → `SurfaceView` (tests only) | the feedback tests render the whole surface view |
| `src/features/library` | `src/features/surfaces` | `library.test.tsx` → `SurfaceView` (tests only) | the library tests render the whole surface view |

Exceptions with a planned fix (A4):

| From | To | Where | Problem | Fix in A4 |
| --- | --- | --- | --- | --- |
| `src/components` | `src/features/context` | `Sidebar.tsx` → `ContextControl` | the shared layer imports a feature | move `Sidebar` to `src/app` |

After A4 the expected composition points are: surfaces → editor, surfaces → library, editor → feedback, editor → library, settings → context, and app → any feature.
