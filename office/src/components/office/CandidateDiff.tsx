import { useCallback, useEffect, useState } from "react";
import {
  type CandidateDiffSummary,
  CoreApiError,
  coreApi,
} from "../../coreApi";
import { Button } from "../ui/Button";
import { EmptyState, LoadingState } from "../ui/Surface";

export function CandidateDiff({
  campaignId,
  candidateId,
}: {
  campaignId: string;
  candidateId: string;
}) {
  const [data, setData] = useState<CandidateDiffSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [unavailable, setUnavailable] = useState(false);
  const [busy, setBusy] = useState(true);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    setUnavailable(false);
    try {
      setData(await coreApi.getCandidateDiff(campaignId, candidateId));
    } catch (cause) {
      setData(null);
      if (cause instanceof CoreApiError && cause.status === 409) {
        setUnavailable(true);
      } else {
        setError(
          cause instanceof CoreApiError
            ? cause.message
            : "Failed to load the candidate diff summary.",
        );
      }
    } finally {
      setBusy(false);
    }
  }, [campaignId, candidateId]);

  useEffect(() => {
    void load();
  }, [load]);

  if (busy && !data) return <LoadingState label="Loading diff summary…" />;
  if (unavailable) {
    return (
      <EmptyState
        title="Diff summary unavailable"
        description="Core cannot resolve this recorded revision in the Project repository. No diff is inferred."
        action={<Button onClick={() => void load()}>Retry</Button>}
      />
    );
  }
  if (error) {
    return (
      <div className="ui-callout ui-callout--danger" role="alert">
        {error} <Button onClick={() => void load()}>Retry</Button>
      </div>
    );
  }
  if (!data) return <LoadingState label="Loading diff summary…" />;

  return (
    <div className="candidate-diff" data-testid="candidate-diff">
      <dl className="detail">
        <dt>Base commit</dt>
        <dd>
          <code title={data.base_commit}>{data.base_commit.slice(0, 12)}</code>
        </dd>
        <dt>Candidate commit</dt>
        <dd>
          <code title={data.commit_sha}>{data.commit_sha.slice(0, 12)}</code>
        </dd>
        <dt>Diff stat</dt>
        <dd>
          <code>{data.diff_stat || "no diff stat recorded"}</code>
        </dd>
      </dl>
      <h4>
        Changed paths ({data.changed_paths.length}
        {data.changed_paths_truncated ? " shown; truncated" : ""})
      </h4>
      {data.changed_paths.length === 0 ? (
        <p className="muted">No changed path is recorded.</p>
      ) : (
        <ul className="changed-paths">
          {data.changed_paths.map((path) => (
            <li key={path}>
              <code>{path}</code>
            </li>
          ))}
        </ul>
      )}
      <p className="muted">{data.source}</p>
    </div>
  );
}
