# History

Plans and working documents of earlier milestones, kept as they were written. They explain why things are the way they are; they do not describe the product today. For that, start at the [documentation index](../README.md).

Nothing here is edited for content, except one status note at the top of `next-phase-2026-10.md` that says, at the release of 0.3.0, which of its sections were done and where that is recorded. When a file moved, its relative links were rewritten so they still resolve, and only that. Paths and file names in their text are those of the time.

| File | What it was | Moved from |
| --- | --- | --- |
| [next-phase-2026-10.md](next-phase-2026-10.md) | The plan for running LaunchDeck on macOS for the first time (written 2026-10-05 at `6a9224e`): build, test, check by hand, continuous integration. Its section 4 checklist is now [manual checks on macOS](../contributing/manual-checks-macos.md), brought up to the current interface; what was done is in the [verification records](../verification/README.md). | `NEXT_PHASE.md` at the root, until `4ba5305` (2026-10-06) |
| [plans/IMPLEMENTATION_PLAN.md](plans/IMPLEMENTATION_PLAN.md) | The original plan of the runtime, devices and interface. | the root, until `4ba5305` |
| [plans/STATE_FEEDBACK_PLAN.md](plans/STATE_FEEDBACK_PLAN.md) | The State Hub and feedback rules. | the root, until `4ba5305` |
| [plans/FEEDBACK_EDITOR_PLAN.md](plans/FEEDBACK_EDITOR_PLAN.md) | The feedback editor. | the root, until `4ba5305` |
| [plans/ACTION_LIBRARY_PLAN.md](plans/ACTION_LIBRARY_PLAN.md) | The Action Library, parameters and availability. | the root, until `4ba5305` |
| [plans/AGENT_CONTROL_PLAN.md](plans/AGENT_CONTROL_PLAN.md) | Agent Control. Its "Agent Pack" (`agents/pack.rs`, `examples/agent-pack/`) was removed during productization; "Pack" now means a shared file of surfaces ([sharing](../user/sharing.md)). | the root, until `4ba5305` |
| [plans/PLUGIN_SDK_PLAN.md](plans/PLUGIN_SDK_PLAN.md) | The WebAssembly plugin SDK and host. | the root, until `4ba5305` |
| [plans/PLUGIN_CAPABILITIES_PLAN.md](plans/PLUGIN_CAPABILITIES_PLAN.md) | The network capability, grants and secrets. | the root, until `4ba5305` |
| [plans/CONTEXT_ENGINE_PLAN.md](plans/CONTEXT_ENGINE_PLAN.md) | Auto Context. | the root, until `4ba5305` |

The verification record of the first macOS run, `VERIFICATION.md` at the root until the same commit, is in [docs/verification/](../verification/README.md), not here: records of what was run stay together.

The two plans of the current milestone, `PRODUCTIZATION_PLAN.md` and `LAUNCHPAD_MINI_MK3_PLAN.md`, stay at the root of the repository until the milestone closes.
