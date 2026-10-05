# MCP bridge (optional, design only)

**Status: not built.** Nothing in this repository speaks the Model Context Protocol, nothing depends on it, and no MCP client has been run against LaunchDeck. This page describes where an MCP bridge would sit, what it could honestly do, and what it must never do, so that the current design does not have to change if it is built later.

Agent Control works without MCP. The integration surface is the [Agent Bridge](bridge-protocol.md) (a localhost WebSocket that any language can use) and the [hook endpoint](security.md#the-hook-endpoint). MCP would be a third, optional way in for agents that already speak it. It would be a translator and nothing more.

## What the specification says today

Read on 2026-10-04 from the MCP specification, revision 2026-07-28, [transports overview](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports):

* There are two standard transports. **stdio** carries newline-delimited messages over the standard streams of a subprocess the client launches. **Streamable HTTP** sends each message as an HTTP POST to a single endpoint and returns a JSON object or a request-scoped event stream.
* In this revision, servers do not initiate JSON-RPC requests and clients do not send JSON-RPC responses. A server cannot ask the client something unprompted, and it cannot push a command to the agent.
* Custom transports are allowed if they keep the message format.

The first point shapes the whole design. The Agent Bridge can push an `agent.command` to a client at any time. MCP cannot. The only way an MCP server can hand a person's decision back to an agent is as the **result of a tool call that the agent made and is waiting on**.

Read on 2026-10-04 from Claude Code's [permission modes page](https://code.claude.com/docs/en/permission-modes): a non-interactive `-p` run without a `--permission-prompt-tool` has no prompt to fall back to. That flag lets an MCP tool answer permission prompts. I did not find its argument and response format in the official documentation that day, and the search results showed a GitHub issue titled "flag has no usage documentation" (anthropics/claude-code #24595), which I did not read. Nothing below assumes a format. It would have to be checked against the installed version before anyone builds on it, as the hook payloads were.

I did not re-read the HTTP security section of the specification this session. This page therefore quotes none of it. The rules it sets for itself are stricter than a local HTTP server needs, and the first one avoids the question: no listening port at all.

## Design

```
MCP client (agent harness)
      |  stdio (launched by the harness, no port)
      v
launchdeck-mcp   (a separate small program, not part of the runtime)
      |  Agent Bridge, protocol 1, ws://127.0.0.1:47821
      v
LaunchDeck runtime  ->  AgentControl  ->  pads, app, notifications
```

* **The runtime gains nothing.** `launchdeck-mcp` is an Agent Bridge client. It registers an agent, sends the messages in [bridge-protocol.md](bridge-protocol.md) and receives commands. All validation, ownership, access, limits and audit stay where they are. There is one model, as in the [Adapter SDK](adapter-sdk.md#one-model-two-forms).
* **stdio only.** The harness launches the program and talks to it over pipes. It opens no port and accepts no connection. A Streamable HTTP form is out of scope for a first version.
* **Identity comes from configuration, not from the model.** The agent id, name, provider and harness are arguments or environment variables set by the person who configures the MCP server. A model cannot rename itself or register as another agent through a tool call.
* **Monitor first.** Registration is read-only, as for every other bridge client. A person grants control in the Agents view, and LaunchDeck remembers it by agent id.
* **Off by default.** It exists only if the person installs it and adds it to a harness's MCP configuration.

## Tools

Three tools, all named for what they do and none for a vendor.

| Tool | Does | Bridge message |
| --- | --- | --- |
| `report_status` | Sets the session's status, task line, progress and detail. | `agent.session.update` |
| `request_approval` | Asks a person to decide and **waits** for the answer. | `agent.approval.request`, then `agent.approval.withdraw` if it gives up |
| `end_session` | Closes the session. | `agent.session.end` |

`report_status` accepts only the statuses a bridge client may set (not `approval_required`, which comes from `request_approval`, and not `offline`). The session id is optional, defaults to one id per server process, and is confined to the agent's own sessions by the same ownership rule as any bridge client.

`request_approval` takes one line of text and an optional tool name. It returns one of three words and never anything that reads as consent by default.

| Result | When |
| --- | --- |
| `approved` | A person pressed APPROVE or Approve in the app, and the translator confirmed the command to LaunchDeck while this call was still waiting. |
| `rejected` | A person pressed REJECT. |
| `no_decision` | Control has not been granted for this agent, the wait expired, the connection to LaunchDeck dropped, or the call was cancelled. The tool result says to treat it as not approved. |

The wait is bounded and the bound is configurable. On expiry the translator withdraws the request and returns `no_decision`. If the agent has not been granted control when it asks, the request is shown in LaunchDeck, the translator returns `no_decision` at once, and the harness falls back to its own prompt. This mirrors what the hook adapters do.

Not offered: tools to pause, resume, cancel, retry or instruct. MCP cannot push those to an agent (see above), and the translator declares only `approve` and `reject`, so the pads for the other commands stay visibly unavailable for MCP-connected agents.

## Reporting is cooperation, not enforcement

This is the most important limit, and the reason the first design choice is to keep MCP optional.

A tool the model chooses to call is a courtesy. If the model never calls `request_approval`, nothing stops the action. If it calls the tool and describes the action inaccurately, a person is approving a sentence, not the action. The text is written by the model, and a model that has been manipulated by content it read can write a harmless-looking sentence for a harmful step.

Three consequences are part of the design.

1. **Everything the model writes is untrusted text.** Task, detail, error and approval messages are length-limited single lines, shown as plain text, never interpreted as markup, never executed, and never written to the audit log or to notifications.
2. **The pads say whose words they are.** For an MCP-connected agent, the card and the approval note show that the description was supplied by the agent. When the description comes from the harness itself (the tool name and input a harness reports through hooks), it is shown as such.
3. **Real enforcement belongs to the harness.** When a harness can route its own permission prompts to an MCP tool, the question carries the harness's description of the action, which the model does not write. That is the only MCP path that would be an enforcement point rather than a courtesy. It is the case to build for first, and the case that needs the format check above.

For Claude Code, the [hook adapter](adapter-sdk.md#claude-code-verified) already gives this: the permission request comes from the harness with the real tool name. MCP would add a path for harnesses that have no hooks. It would not replace the hook path where one exists.

## Rules it inherits

These are the platform's rules, and an MCP bridge has no exceptions to them.

* LaunchDeck never accepts arbitrary remote commands. The translator reaches LaunchDeck only through the bridge, on loopback, and can do only what a bridge client can do: describe its own agent.
* Registration is explicit and read-only until a person grants control.
* A permission request is never approved automatically. Timeouts, errors and disconnects end in `no_decision`.
* Critical interventions are audited by the runtime exactly as for any other agent. The translator adds no logging of prompts or payloads.
* Third-party MCP servers are not trusted to be `launchdeck-mcp`. Only the program the person installed registers through it, and the person's grant is tied to its agent id.

## What MCP would add, honestly

| It would give | It would not give |
| --- | --- |
| A way for any MCP-capable agent to appear in Agent Control with two lines of configuration. | Pause, resume, cancel, retry or send_instruction. |
| A way to ask a human before a step the agent chooses to flag. | A guarantee that the agent asks. |
| A vendor-neutral name for those tools. | Observation of work the agent does not report. |

## Should it be built?

Not yet. The hook endpoint covers Claude Code and Codex approvals with real descriptions, and the Agent Bridge covers any program that can open a WebSocket. The case to wait for is a harness that supports an MCP permission-prompt tool with a documented format. Build it then, as a translator, with the tests below.

If built, it would be tested the way the rest of Agent Control is: a real `AgentControl` and bridge in the test process, a real translator connected to them, and assertions on the resulting agent state, approval lifecycle, audit entries and the virtual Launchpad's frame. It would need cases for approval granted, rejected, expired, control not granted, connection lost mid-wait, and a model-supplied status that the bridge refuses.

## Sharing a translator

A translator is adapter code, so it would ship as a program that speaks the Agent Bridge. The [Pack file format](../artifacts/FORMAT.md) carries no adapters in its first version; adapters are a candidate for a later one (see [adapter-sdk.md](adapter-sdk.md#sharing-adapters-not-built)). This is a statement of where it would fit, not a feature.

## Platform ideas outside Agent Control

[roadmap/mcp.md](../roadmap/mcp.md) keeps the older, platform-level thoughts: reading surfaces and proposing one for review. They are independent of agents and equally unbuilt.
