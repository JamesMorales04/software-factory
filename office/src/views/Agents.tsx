// Agents view: S1 ships no Worker or agent runtime, so this view reports
// the honest empty state instead of fabricating sessions. F1-S2 introduces
// real Worker dispatch and observable status here.
export function Agents() {
  return (
    <div>
      <h2>Agents</h2>
      <p className="muted">
        No agents connected. Worker dispatch, leases and observable Run status
        arrive with F1-S2; this view will render real Core–Worker state then —
        never placeholder activity.
      </p>
    </div>
  );
}
