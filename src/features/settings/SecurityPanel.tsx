/** Settings > Security: what can run things on this computer, and what cannot. */
export function SecurityPanel() {
  return (
    <section className="panel" aria-labelledby="security-title">
      <h2 id="security-title">Security</h2>
      <ul className="plain bullets">
        <li>
          Shell commands and HTTP requests only run from pads you configured, and are marked <span className="pill warn">Privileged</span> in the editor.
        </li>
        <li>Events received over the WebSocket can change state. They cannot start actions or send commands.</li>
        <li>The event socket accepts only local connections from this machine.</li>
        <li>
          The Agent Bridge is not authenticated: any program on this computer can register an agent on it. Such an agent can only be watched until you allow
          control of it in Agents.
        </li>
      </ul>
    </section>
  );
}
