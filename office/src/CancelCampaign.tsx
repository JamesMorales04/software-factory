import { useState } from "react";
import { Button } from "./components/ui/Button";
import { Input } from "./components/ui/Field";
import { type Campaign, CoreApiError, coreApi } from "./coreApi";
import { shortId } from "./format";

const TERMINAL = new Set(["CANCELLED", "COMPLETED", "FAILED"]);

export function isCampaignTerminal(status: string): boolean {
  return TERMINAL.has(status);
}

interface CancelCampaignProps {
  campaign: Campaign;
  disabled?: boolean;
  onCancelled: (campaign: Campaign) => void | Promise<void>;
}

export function CancelCampaign({
  campaign,
  disabled = false,
  onCancelled,
}: CancelCampaignProps) {
  const [confirming, setConfirming] = useState(false);
  const [reason, setReason] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (isCampaignTerminal(campaign.status)) {
    return null;
  }

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const cancelled = await coreApi.cancelCampaign(
        campaign.id,
        reason.trim(),
      );
      setConfirming(false);
      setReason("");
      await onCancelled(cancelled);
    } catch (cause) {
      setError(
        cause instanceof CoreApiError
          ? cause.message
          : "Campaign cancellation failed.",
      );
    } finally {
      setBusy(false);
    }
  }

  if (!confirming) {
    return (
      <Button
        variant="danger"
        size="sm"
        disabled={disabled || busy}
        onClick={() => {
          setError(null);
          setConfirming(true);
        }}
      >
        Cancel Campaign
      </Button>
    );
  }

  return (
    <form
      className="cancel-confirm"
      aria-label={`Confirm cancellation of Campaign ${shortId(campaign.id)}`}
      onSubmit={submit}
    >
      <p>
        Cancel <code>{shortId(campaign.id)}</code>? Core will request
        cancellation, mark queued work cancelled and flag active provider
        sessions. External provider termination may remain unconfirmed.
      </p>
      <Input
        data-testid="cancel-reason"
        label="Reason (optional)"
        value={reason}
        maxLength={500}
        onChange={(event) => setReason(event.target.value)}
      />
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="row">
        <Button type="submit" variant="danger" disabled={busy}>
          {busy ? "Cancelling…" : "Confirm cancellation"}
        </Button>
        <Button
          type="button"
          disabled={busy}
          onClick={() => {
            setConfirming(false);
            setError(null);
          }}
        >
          Keep Campaign
        </Button>
      </div>
    </form>
  );
}
