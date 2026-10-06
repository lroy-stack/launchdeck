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
| `src/app` | `App.tsx`: the window shell, the first run in its place while the onboarding flag is off (or the import view, when the first run opens it), the switch between views, the toasts (a failure's sentence, its detail under Details; an error with a detail stays until dismissed) and the fatal screen (the sentence, the detail under Details); `StartupNotices.tsx` (a banner per notice of the snapshot: the saved workspace could not be opened, with Show folder and the backup's path under Details); `Sidebar.tsx` (navigation: Overview, Agents, Surfaces, Plugins, Settings; the device card, surfaces with their Imported badge, Import… and Export…, the Auto Context control) | everything |
| `src/components` | `PadGrid.tsx` (the pads of a page, drawn by the surface view and the Agents view's Agent Control surface tab); `ActivityFeed.tsx` (recent events, on Home and in the live panel); `DeviceStatusLine.tsx` (a device's name, status in words and problem, or a connected device's notice, in the sidebar, on Home and in Settings); `forms/ParamForm.tsx` (a form drawn from a parameter schema, an action's or an observable's) and `forms/ColorField.tsx` (a colour as a picker, a hex field and swatches), which the Inspector and the feedback editor share; `Problem.tsx` (`ProblemText`: a failure as Rust worded it, the sentence and its detail behind Details, in the toasts, the fatal screen and every view that shows a refusal; `Disclosure`, a Details disclosure whose content is in the page only while open) | `src/lib`, `src/stores` |
| `src/features/agents` | the Agents view: `AgentsView`, `Overview`, `Sessions`, `Adapters`, `SurfaceTab` and their `parts` | shared |
| `src/features/artifacts` | Surface files and Packs: `ImportView` (choose a file, Rust's plan or refusal, apply, the report; opened from the first run, it offers Back to the welcome and an applied import sets the onboarding flag), `PlanSections` (the sections of an import plan, each choice sent back whole as decisions; Rust computes ids, conflicts, dependencies and validity), `Questions` (a Pack's inputs as fields), `useAsk` (ask Rust again with only the latest question, typing waits for a pause), `ExportView` (choose, preview, save, show), `ExportParts` (its sections and Rust's preview), `request` (the person's choices as an `ExportRequest`), `text` (the wording), `contract.test.ts` (the artifact replies of `examples/artifacts`, parsed with the schemas). Opened by `src/app` only: the surface view's Export… goes through the store (`startExport`), not through an import | shared |
| `src/features/context` | Auto Context: `ContextPanel` (rules, fallback, status), `ContextControl` (the sidebar switch), `text.ts` (the wording) | shared |
| `src/features/editor` | the Inspector: `Inspector`, `AssignmentEditor` | shared; `src/features/feedback`, `src/features/library` |
| `src/features/feedback` | the feedback editor: `FeedbackEditor`, `StateRules`, `EventRules`, `LooksEditor`, `LookSelect`, `ObservablePicker`, `LiveFeedback`, `useReview`, `parts`, and the test helper `testkit.tsx` | shared |
| `src/features/home` | `HomeView`: the Overview view (Agent Control, the surfaces with the descriptions Rust wrote, devices, recent activity); the sidebar calls it Overview so that "Home" is only the Home starter | shared |
| `src/features/library` | the Action Library panel: `ActionLibrary`, `parts` | shared |
| `src/features/onboarding` | `FirstRun`: the one screen `App` shows instead of the shell while `preferences.onboardingDone` is false: the device status with Look again, the purpose radiogroup (React holds only the labels and the starter ids; text and icon are `Surface.description` and `Surface.icon`), Start (activation and Live, or Build my own: `create_surface`, Edit, the Action Library focused), Skip, and Import a Pack instead (the import view, through the store; `App` shows it); `set_onboarding_done(true)` is the last call | shared |
| `src/features/plugins` | the Plugins view: `PluginsView` (installed plugins, the empty state, the example plugins the app ships with Install), `Access` (network grants and secrets), `LegacySecret` (the "Legacy insecure secret" card: Migrate or Remove a credential kept in plain text), `text` (the sentence for each kind of plugin problem, Rust decides the kind; the empty state and the example plugins' wording) | shared |
| `src/features/settings` | `SettingsView`: the sections in order with a list that leads to each, and one `app_info` read for About and Developer. `DevicesPanel` (status lines, Look again, Connect and Disconnect, ports and capabilities under Technical details), `DeviceSettings` (a connected Launchpad's line with firmware and Programmer Mode, Sleep or Wake, Scroll a test text, what it can do in words; and the one brightness for every device that has it, sent through `set_device_brightness`, never with the other preferences), Auto Context (`ContextPanel`), `NotificationsPanel` (Agent Control's notifications switch, moved from Agents > Adapters; the hook endpoint stays there), `PluginsPanel` (where plugin secrets are kept, Show plugins folder, Open Plugins), `SecurityPanel`, `WorkspacePanel` (Import a Surface or Pack… and Export…, which open the import and export views through the store; reset after a confirmation), `DeveloperPanel` (closed until opened, its tools drawn only then: the external events WebSocket and test sender, the data folder with Show, Show the welcome again), `AboutPanel` (every version `app_info` reports, the license, Copy for a bug report); the words in `text` | shared; `src/features/context` |
| `src/features/surfaces` | the surface view: `SurfaceView` (with Export…, which opens the export view through the store), `LivePanel`, `SurfaceManage` (in Edit: the surface's name, description and icon in the header, Delete surface with its confirmation, each page's menu to rename or delete it) | shared; `src/features/editor`, `src/features/library` |
| `src/lib` | `ipc.ts` (the only bridge to Rust; `api`, events, every reply parsed with Zod; `toProblem`, any rejection as `{message, detail?}`, and the one UI-worded failure, a reply its schema refuses), `schema/` (the Zod schemas, one file per domain: `actions`, `observables`, `feedback`, `workspace`, `context`, `runtime` for devices, runtime state, snapshot, frames and the app's versions (`appInfoSchema`), `plugins` for plugins, network access, secrets and the example plugins, `artifacts` for Surface files and Packs (the import plan and its outcomes, the export request and preview, the limits, a surface's `importedFrom`); `schema/index.ts` re-exports them all, so everything imports `./schema`), `agents.ts` (Agent Control schemas and helpers), `preview.ts` (a browser-only stand-in for the runtime, fed by the contract fixtures), `actions.ts` (catalog index, the `normalizeArgs` mirror), `feedbackEdit.ts` (the editor's pure functions, the `operatorsFor` and `problemsOf` mirrors), `devices.ts` (a device's status, summary line, notice and capabilities in words, `primaryDevice`), `observables.ts`, `assign.ts`, `color.ts` and `format.ts` (times, event sentences, plurals, and `categoryLabel`: a surface category in words, never its slug) | nothing outside `src/lib` (and the fixtures in `examples/`) |
| `src/stores` | `runtime.ts`: the Zustand store. It mirrors what Rust pushes (snapshot, catalog, agents, context, the start's notices) and keeps purely visual choices (view, selection, toasts, the surfaces the export view opens with); pure selectors over it, such as `primaryDeviceOf` | `src/lib` |

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

Tests of a feature may also render `src/app/Sidebar.tsx`: `context/context.test.tsx`, `plugins/access.test.tsx` and `settings/devices.test.tsx` do, to test the Auto Context switch, the badge on Plugins and the device card where a person sees them.
