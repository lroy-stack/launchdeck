# Generic agent (Agent Bridge example)

A complete, dependency-free client for the [Agent Bridge](../../docs/agent-control/bridge-protocol.md). It plays a small agent that updates a changelog and shows every part of the protocol once:

1. connects to the local WebSocket and registers (`agent.register`)
2. creates a session and reports progress (`agent.session.update`)
3. asks permission before a risky step (`agent.approval.request`)
4. receives the human's decision as an `agent.command` and answers it (`agent.command.result`)
5. obeys `pause`, `resume` and `cancel`, then finishes and unregisters

It never approves anything itself. If nobody answers within the timeout it withdraws the request and ends the session as failed.

## Run it

You need Node 22.18 or newer (older 22.x: add `--experimental-strip-types`). There is nothing to install.

```sh
# 1. start LaunchDeck (the desktop app, or the windowless runner)
npm run tauri dev
#    or: cargo run --manifest-path src-tauri/Cargo.toml --example headless

# 2. in another terminal
node examples/generic-agent/agent.ts
```

Open **Agents** in the app. The example appears as "Example agent", read-only: registering never grants control, so the pads for Approve, Reject, Pause and Stop stay dim.

1. Under **Overview**, find the card and choose **Allow control**. You can do this at any moment, even after the approval request has arrived.
2. The APPROVE pad now flashes, on the virtual Launchpad and on a connected Launchpad X.
3. Press **Approve** (pad or app). The client logs `approved: continuing`, finishes the changelog and completes.

Try **Reject**, **Pause** and **Stop** too. Each one arrives as an `agent.command` and is acknowledged with its `requestId`. If nobody decides within `APPROVAL_TIMEOUT_SECS` the client withdraws the request and fails the session rather than assuming consent.

## Options

| Variable | Default | Meaning |
| --- | --- | --- |
| `LAUNCHDECK_URL` | `ws://127.0.0.1:47821` | Where the bridge listens (shown on the Adapters tab). |
| `AGENT_ID` | `example-agent` | Stable id. Run two copies with different ids to see two agents side by side. |
| `SESSION_ID` | `<AGENT_ID>-1` | Session id. |
| `STEP_MS` | `1200` | Pace of the simulated work. |
| `APPROVAL_TIMEOUT_SECS` | `120` | How long to wait for a decision before giving up. |
| `LINGER_SECS` | `20` | How long to stay registered after finishing, so the result stays visible. |

## Use it as a template

`agent.ts` is about 190 lines and each protocol message is sent from one obvious place. To connect your own tool, keep `register`, replace `run()` with the real work, report status through `report(...)`, and keep `onCommand` honest: only declare capabilities you can really carry out, and answer every command with `ok: true` or `ok: false` plus a short `error`.
