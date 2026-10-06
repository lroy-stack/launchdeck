# MCP (roadmap)

**Not built.** LaunchDeck does not implement the Model Context Protocol. This page keeps the platform-level ideas. The part that concerns agents has its own document, with the current design, its limits and the research behind it: [the MCP bridge](mcp-bridge.md).

## Where MCP would sit

MCP is an adapter on the edge of the runtime, like the WebSocket interface. For agents it would be a translator in front of the [Agent Bridge](../agent-control/bridge-protocol.md), not a second model, and not part of the runtime. It must never become a way for a model to press your pads, run shell commands or change what a pad does without the user seeing it.

```
MCP client (agent)  <--stdio-->  launchdeck-mcp  <--Agent Bridge, 127.0.0.1-->  Agent Control
```

## Agent status and approval

Covered in [the MCP bridge](mcp-bridge.md): three tools (`report_status`, `request_approval`, `end_session`), identity fixed by configuration, monitor-first, bounded waits that end in "no decision", and the plain statement that a tool the model chooses to call is cooperation, not enforcement.

The earlier idea of normalising agent status to `task.*` events is gone. Agent Control publishes its own semantic events with `source: "agent-control"`, and the generic `event` protocol cannot use that source.

## Ideas that are independent of agents

These were part of the first sketch and remain unbuilt. None of them can be started without a review step in the app.

| Primitive | Name | Behaviour |
| --- | --- | --- |
| Resource | `launchdeck://surfaces` | Read-only list of surfaces and pages. |
| Resource | `launchdeck://state` | Current surface, page, mode and control states. |
| Tool | `launchdeck.propose_surface` | Submit a surface JSON for the user to review. It is never applied automatically. |

Deliberately absent: pressing a pad, running an action, editing a control, reading shell command text from a surface without the user's say-so.

## Security requirements

* Transport: stdio or localhost only. No remote MCP.
* Each client registers a name and gets only the `source` it registered, so one client cannot impersonate another, `runtime`, `device` or `agent-control`.
* Writes are limited to events and, for agents, to the bridge's report-only messages. Anything that changes a surface goes through a review screen in the app, with privileged actions listed.
* Rate limits and the same 64 KiB message cap.
* An off switch in Settings, off by default.

## Out of scope for a first iteration

Sampling, prompts, remote servers, authentication flows, and any tool that mutates the workspace without confirmation.
