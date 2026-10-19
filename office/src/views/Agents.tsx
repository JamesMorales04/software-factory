// Agents view: S2 ships Worker dispatch with observable Run status under
// Runs/Workers — real Core–Worker state, never placeholder activity.
// Autonomous Paseo agent sessions remain blocked by SEC-01, so this view
// keeps reporting the honest empty state instead of fabricating sessions.
export function Agents() {
  return (
    <div>
      <h2>Agents</h2>
      <p className="muted">
        No autonomous agent sessions connected (SEC-01 blocked). Worker
        execution and lease state live under Runs and Workers — rendered from
        authoritative Core snapshots, never placeholder activity.
      </p>
    </div>
  );
}
