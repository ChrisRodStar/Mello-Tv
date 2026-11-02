# Playback recovery regression checks

Run `bun run test:playback` with Google Chrome installed. The script starts an ephemeral loopback-only Vite fixture, runs the production player and API resolver in React Strict Mode, and closes the browser/server afterward.

All API and stream responses are intercepted. External requests are blocked. The 29 KB MP4 is a generated, silent, 30-second gray video; it contains no provider media or household data.

Checks cover natural episode end followed by a rejected stream, one automatic fresh refresh per episode, bounded retries, manual fresh retry, position preservation, duplicate media errors, retry idempotency after a network error, and cancelling recovery by changing episodes. No live provider is contacted.

These checks supplement the unit/API tests and do not establish real-provider or Safari playback behavior.
