// A complete, dependency-free Agent Bridge client.
//
// It plays a small agent: registers, starts a session, reports progress, asks
// for approval before a "risky" step, obeys approve / reject / pause / resume /
// cancel commands from LaunchDeck, and finishes. It never decides for itself
// that a request is approved: no answer means it gives up and fails.
//
//   node agent.ts                    (Node 22.18+; older 22.x: --experimental-strip-types)
//   LAUNCHDECK_URL=ws://127.0.0.1:47821 AGENT_ID=my-agent node agent.ts
//
// Protocol reference: docs/agent-control/bridge-protocol.md

const URL = process.env.LAUNCHDECK_URL ?? "ws://127.0.0.1:47821";
const AGENT_ID = process.env.AGENT_ID ?? "example-agent";
const SESSION_ID = process.env.SESSION_ID ?? `${AGENT_ID}-1`;
const STEP_MS = Number(process.env.STEP_MS ?? 1200);
const APPROVAL_TIMEOUT_MS = Number(process.env.APPROVAL_TIMEOUT_SECS ?? 120) * 1000;
const LINGER_MS = Number(process.env.LINGER_SECS ?? 20) * 1000;

type Json = Record<string, unknown>;

const ws = new WebSocket(URL);
const log = (...a: unknown[]) => console.log(new Date().toISOString().slice(11, 19), ...a);
const send = (msg: Json) => ws.send(JSON.stringify(msg));
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

// --- what this "agent" is doing ---------------------------------------------

let paused = false;
let cancelled = false;
let progress = 0;
let access = "monitor";
let decision: ((d: "approved" | "rejected" | "gave-up") => void) | null = null;

function report(status: string, extra: Json = {}) {
  send({
    type: "agent.session.update",
    agentId: AGENT_ID,
    session: { id: SESSION_ID, status, task: "Update the changelog", progress, ...extra },
  });
}

/** Do one unit of work, honouring pause and cancel. */
async function work(detail: string, to: number) {
  while (progress < to && !cancelled) {
    if (paused) {
      await sleep(200);
      continue;
    }
    progress = Math.min(to, progress + 0.05);
    report("running", { detail });
    await sleep(STEP_MS / 4);
  }
}

async function run() {
  report("starting");
  await sleep(STEP_MS);
  await work("Reading the repository", 0.4);
  if (cancelled) return;

  // A step that needs a human. Ask, then wait. Nothing proceeds without a yes.
  log("asking for approval (open LaunchDeck and press APPROVE or REJECT)");
  send({
    type: "agent.approval.request",
    agentId: AGENT_ID,
    sessionId: SESSION_ID,
    approval: { id: "write-changelog", message: "Write release notes to CHANGELOG.md?", tool: "fs.write" },
  });
  if (access !== "control") {
    log("LaunchDeck only monitors this agent. Choose Allow control on its card in Agents > Overview to answer from the pads.");
  }
  const answer = await new Promise<"approved" | "rejected" | "gave-up">((resolve) => {
    decision = resolve;
    setTimeout(() => resolve("gave-up"), APPROVAL_TIMEOUT_MS);
  });
  decision = null;

  if (answer === "gave-up") {
    log("no decision in time: withdrawing the request and not performing the step");
    send({ type: "agent.approval.withdraw", agentId: AGENT_ID, sessionId: SESSION_ID, approvalId: "write-changelog" });
    report("failed", { error: "No decision was made, so the step was not performed." });
    return;
  }
  if (answer === "rejected") {
    log("rejected: skipping the write");
    report("waiting", { detail: "Rejected; waiting for new instructions" });
    await sleep(STEP_MS * 2);
    report("cancelled");
    return;
  }
  log("approved: continuing");
  await work("Writing CHANGELOG.md", 1);
  if (cancelled) return;
  report("completed", { detail: "Changelog updated" });
  log("done");
}

// --- commands from LaunchDeck ----------------------------------------------

interface CommandFrame {
  requestId: string;
  command?: { kind?: string; text?: string };
}

function onCommand(m: CommandFrame) {
  const kind = m.command?.kind ?? "";
  const ok = (extra: Json = {}) =>
    send({ type: "agent.command.result", agentId: AGENT_ID, requestId: m.requestId, ok: true, ...extra });
  const fail = (error: string) =>
    send({ type: "agent.command.result", agentId: AGENT_ID, requestId: m.requestId, ok: false, error });

  log("command:", kind);
  switch (kind) {
    case "approve":
    case "reject":
      if (!decision) return fail("nothing is waiting for a decision");
      ok();
      decision(kind === "approve" ? "approved" : "rejected");
      break;
    case "pause":
      paused = true;
      ok();
      report("paused");
      break;
    case "resume":
      paused = false;
      ok();
      report("running");
      break;
    case "cancel":
      cancelled = true;
      paused = false;
      ok();
      decision?.("rejected");
      report("cancelled");
      break;
    default:
      fail(`this agent does not support ${kind}`);
  }
}

// --- wiring ------------------------------------------------------------------

ws.addEventListener("open", () => {
  log("connected to", URL);
  send({
    type: "agent.register",
    protocol: 1,
    agent: {
      id: AGENT_ID,
      name: "Example agent",
      provider: "example",
      harness: "node-script",
      // Only declare what you will really do. LaunchDeck greys out the rest.
      capabilities: ["approve", "reject", "pause", "resume", "cancel"],
    },
  });
});

ws.addEventListener("message", (event) => {
  const m = JSON.parse(String(event.data));
  switch (m.type) {
    case "agent.registered":
      access = m.access;
      log(`registered (access: ${m.access}, protocol ${m.protocol})`);
      run()
        .then(() => sleep(LINGER_MS))
        .then(() => {
          send({ type: "agent.unregister", agentId: AGENT_ID });
          ws.close();
        });
      break;
    case "agent.access":
      access = m.access;
      log("access is now", m.access);
      break;
    case "agent.command":
      onCommand(m);
      break;
    case "error":
      log("LaunchDeck says:", m.code, m.message);
      break;
    default: // acks
      break;
  }
});

ws.addEventListener("close", () => {
  log("disconnected");
  process.exit(0);
});
ws.addEventListener("error", () => {
  console.error(`Could not reach ${URL}. Is LaunchDeck running with external events enabled?`);
  process.exit(1);
});
