import { useId, useState } from "react";
import { CoreApiError, coreApi } from "../../coreApi";
import { useI18n } from "../../i18n";
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
  const { t } = useI18n();
  const actorId = useId();
  const reasonId = useId();
  const [actor, setActor] = useState("office-operator");
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    if (!reason.trim()) {
      setError(t("modelResolution.reasonRequired"));
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
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : t("modelResolution.error"),
      );
      setBusy(false);
      return;
    }
    setReason("");
    try {
      await onResolved();
    } catch {
      setError(t("modelResolution.refreshError"));
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
        label={t("modelResolution.operator")}
        value={actor}
        onChange={(event) => setActor(event.target.value)}
        autoComplete="username"
        className="decision-resolve-field"
      />
      <Input
        id={reasonId}
        label={t("modelResolution.reason")}
        value={reason}
        onChange={(event) => setReason(event.target.value)}
        placeholder={t("modelResolution.reasonPlaceholder")}
        className="decision-resolve-reason"
      />
      <Button
        variant="primary"
        size="sm"
        onClick={() => void submit()}
        disabled={busy || !reason.trim()}
      >
        {busy ? t("modelResolution.releasing") : t("modelResolution.release")}
      </Button>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
