import { useId, useState } from "react";
import { CoreApiError, coreApi } from "../../coreApi";
import { Button } from "../ui/Button";
import { Input } from "../ui/Field";

export function ModelCallResolution({
  modelCallId,
  onResolved,
  compact = false,
}: {
  modelCallId: string;
  onResolved: () => Promise<void>;
  compact?: boolean;
}) {
  const actorId = useId();
  const reasonId = useId();
  const [actor, setActor] = useState("office-operator");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    if (!reason.trim()) {
      setError("A reason is required.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await coreApi.resolveModelCall(
        modelCallId,
        actor.trim() || "office-operator",
        reason.trim(),
      );
      setReason("");
      await onResolved();
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Failed to release the model reservation.",
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <div
      className={
        compact
          ? "decision-resolve decision-resolve--compact"
          : "decision-resolve"
      }
    >
      <Input
        id={actorId}
        label="Operator"
        value={actor}
        onChange={(event) => setActor(event.target.value)}
        autoComplete="username"
        className="decision-resolve-field"
      />
      <Input
        id={reasonId}
        label="Release reason"
        value={reason}
        onChange={(event) => setReason(event.target.value)}
        placeholder="Why are you releasing this reservation?"
        className="decision-resolve-reason"
      />
      <Button
        variant="primary"
        size="sm"
        onClick={() => void submit()}
        disabled={busy || !reason.trim()}
      >
        {busy ? "Releasing…" : "Release reservation"}
      </Button>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
