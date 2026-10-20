import type { StreamState } from "../useEventStream";

const LABELS: Record<StreamState, string> = {
  connecting: "Connecting",
  live: "Connected",
  reconnecting: "Reconnecting",
  error: "Connection unavailable",
};

/**
 * Visible transport state. A disconnected UI never means an agent stopped:
 * the label describes this browser's event stream only.
 */
export function StreamStateBadge({ state }: { state: StreamState }) {
  return (
    <span className={`pill stream-${state}`} role="status" aria-live="polite">
      {LABELS[state]}
    </span>
  );
}
