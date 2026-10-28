import { Check, Copy, FileDiff, WrapText } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  type CandidateDiffSummary,
  type CandidateFileDiff,
  CoreApiError,
  coreApi,
} from "../../coreApi";
import { Button } from "../ui/Button";
import { EmptyState, LoadingState } from "../ui/Surface";

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function omissionLabel(file: CandidateFileDiff): string | null {
  switch (file.omitted) {
    case "BINARY":
      return "Binary file; Core does not render a text patch.";
    case "TOO_LARGE":
      return "File exceeds Core's 512 KiB text bound; no patch is rendered.";
    case "NOT_A_FILE":
      return "Symlink or gitlink; Core does not follow it.";
    case "NON_UTF8":
      return "Patch is not valid UTF-8; Core does not guess an encoding.";
    case "BUDGET":
      return "Patch omitted because the total diff byte budget was reached.";
    default:
      return null;
  }
}

function statusLabel(status: CandidateFileDiff["status"]): string {
  if (status === "ADDED") return "added";
  if (status === "DELETED") return "deleted";
  return "modified";
}

function CopyButton({
  label,
  value,
  disabled = false,
}: {
  label: string;
  value: string;
  disabled?: boolean;
}) {
  const [state, setState] = useState<"idle" | "copied" | "unavailable">("idle");

  async function copy() {
    if (!navigator.clipboard?.writeText) {
      setState("unavailable");
      return;
    }
    try {
      await navigator.clipboard.writeText(value);
      setState("copied");
    } catch {
      setState("unavailable");
    }
  }

  return (
    <span className="diff-copy">
      <Button
        size="sm"
        variant="quiet"
        onClick={() => void copy()}
        disabled={disabled}
        aria-label={label}
      >
        {state === "copied" ? (
          <Check size={14} aria-hidden="true" />
        ) : (
          <Copy size={14} aria-hidden="true" />
        )}
        {state === "copied" ? "Copied" : "Copy"}
      </Button>
      <span className="muted" role="status" aria-live="polite">
        {state === "unavailable"
          ? "Clipboard unavailable in this browser context."
          : ""}
      </span>
    </span>
  );
}

function RevisionFact({
  label,
  value,
  copyLabel,
}: {
  label: string;
  value: string;
  copyLabel: string;
}) {
  return (
    <div className="diff-revision-fact">
      <dt>{label}</dt>
      <dd>
        <code title={value}>{value.slice(0, 12)}</code>
        <CopyButton label={copyLabel} value={value} />
      </dd>
    </div>
  );
}

function DiffCode({ file, wrap }: { file: CandidateFileDiff; wrap: boolean }) {
  const lines = useMemo(() => {
    const patch = file.patch ?? "";
    const projected: { text: string; offset: number }[] = [];
    let offset = 0;
    for (const text of patch.split("\n")) {
      projected.push({ text, offset });
      offset += text.length + 1;
    }
    return { patch, projected };
  }, [file.patch]);
  if (!file.patch) return null;
  return (
    <section
      className={`diff-code-shell${wrap ? " diff-code-shell--wrap" : ""}`}
      aria-label={`Unified diff for ${file.path}`}
      // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable diff region must be focusable so its overflow can be scrolled from the keyboard.
      tabIndex={0}
    >
      <pre className={`diff-code${wrap ? " diff-code--wrap" : ""}`}>
        <code>
          {lines.projected.map((line) => {
            let tone = "context";
            if (
              line.text.startsWith("diff --git") ||
              line.text.startsWith("index ") ||
              line.text.startsWith("new file mode") ||
              line.text.startsWith("deleted file mode") ||
              line.text.startsWith("similarity index") ||
              line.text.startsWith("--- ") ||
              line.text.startsWith("+++ ")
            ) {
              tone = "meta";
            } else if (line.text.startsWith("@@")) {
              tone = "hunk";
            } else if (line.text.startsWith("+")) {
              tone = "add";
            } else if (line.text.startsWith("-")) {
              tone = "del";
            }
            return (
              <span
                className={`diff-line diff-line--${tone}`}
                key={`${line.offset}-${line.text.slice(0, 24)}`}
              >
                {line.text}
                {line.offset + line.text.length < lines.patch.length
                  ? "\n"
                  : ""}
              </span>
            );
          })}
        </code>
      </pre>
    </section>
  );
}

export function CandidateDiff({
  campaignId,
  candidateId,
}: {
  campaignId: string;
  candidateId: string;
}) {
  const [data, setData] = useState<CandidateDiffSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [errorKind, setErrorKind] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [wrap, setWrap] = useState(false);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    setErrorKind(null);
    try {
      const projection = await coreApi.getCandidateDiff(
        campaignId,
        candidateId,
      );
      setData(projection);
      setSelectedPath((current) =>
        projection.files.some((file) => file.path === current)
          ? current
          : (projection.files[0]?.path ?? null),
      );
    } catch (cause) {
      setData(null);
      if (cause instanceof CoreApiError) {
        setError(cause.message);
        const body =
          typeof cause.body === "object" && cause.body !== null
            ? (cause.body as Record<string, unknown>)
            : null;
        setErrorKind(body && typeof body.kind === "string" ? body.kind : null);
      } else {
        setError("Failed to load the candidate diff.");
      }
    } finally {
      setBusy(false);
    }
  }, [campaignId, candidateId]);

  useEffect(() => {
    void load();
  }, [load]);

  const selected =
    data?.files.find((file) => file.path === selectedPath) ?? null;

  if (busy && !data)
    return <LoadingState label="Loading candidate file diff…" />;
  if (error) {
    return (
      <div className="ui-callout ui-callout--danger" role="alert">
        <strong>Diff unavailable.</strong> {error}
        {errorKind && <span className="muted"> ({errorKind})</span>}{" "}
        <Button size="sm" onClick={() => void load()}>
          Retry
        </Button>
      </div>
    );
  }
  if (!data) return <LoadingState label="Loading candidate file diff…" />;

  const omittedFiles = data.files.filter((file) => file.omitted).length;
  const gateLabel = data.gate ? data.gate.outcome : "No Gate decision recorded";

  return (
    <div className="candidate-diff" data-testid="candidate-diff">
      <div className="diff-heading">
        <div>
          <h3>
            <FileDiff size={17} aria-hidden="true" /> Changed files
          </h3>
          <p className="muted">
            Exact frozen base to exact Candidate commit. Core reads Git objects;
            it does not execute the Candidate.
          </p>
        </div>
        <section className="diff-totals" aria-label="Diff totals">
          <span>
            <strong>{data.returned_files}</strong>{" "}
            {data.files_truncated
              ? "shown"
              : data.returned_files === 1
                ? "file"
                : "files"}
          </span>
          <span className="diff-add">+{data.total_additions}</span>
          <span className="diff-del">−{data.total_deletions}</span>
          <span className="muted">{formatBytes(data.patch_bytes)} patch</span>
        </section>
      </div>

      <dl className="diff-revisions">
        <RevisionFact
          label="Base commit"
          value={data.base_commit}
          copyLabel="Copy base commit SHA"
        />
        <RevisionFact
          label="Candidate commit"
          value={data.commit_sha}
          copyLabel="Copy candidate commit SHA"
        />
        <RevisionFact
          label="Candidate tree"
          value={data.tree_sha}
          copyLabel="Copy candidate tree SHA"
        />
        <div className="diff-revision-fact">
          <dt>Candidate</dt>
          <dd>
            <code title={data.candidate.id}>
              {data.candidate.id.slice(0, 12)}
            </code>{" "}
            <span className="muted">
              implementer {data.candidate.implementer_id || "not recorded"}
            </span>
          </dd>
        </div>
        <div className="diff-revision-fact">
          <dt>Gate identity</dt>
          <dd>
            <span
              className={`diff-gate diff-gate--${data.gate ? data.gate.outcome.toLowerCase() : "none"}`}
            >
              {gateLabel}
            </span>
            {data.gate && (
              <span className="muted">
                {" "}
                · {data.gate.candidate_id.slice(0, 12)}
              </span>
            )}
          </dd>
        </div>
      </dl>

      {data.files_truncated && (
        <div className="ui-callout ui-callout--attention" role="status">
          Core returned only the first {data.returned_files} changed files. The
          path list is truncated at the Core bound; no missing file is inferred.
        </div>
      )}
      {data.patch_truncated && (
        <div className="ui-callout ui-callout--attention" role="status">
          At least one patch was truncated at Core's byte bound. The truncated
          text is labelled per file.
        </div>
      )}

      <p className="diff-stat">
        <span className="muted">Git diff stat: </span>
        <code>{data.diff_stat || "no diff stat recorded"}</code>
      </p>

      {data.files.length === 0 ? (
        <EmptyState
          title="No changed files"
          description="Core resolved both revisions but the candidate tree has no file difference from the recorded base."
        />
      ) : (
        <div className="diff-workspace">
          <section className="diff-file-list" aria-label="Changed files">
            {data.files.map((file) => (
              <button
                type="button"
                key={file.path}
                className={`diff-file${file.path === selectedPath ? " diff-file--selected" : ""}`}
                onClick={() => setSelectedPath(file.path)}
                aria-current={file.path === selectedPath ? "true" : undefined}
              >
                <span
                  className={`diff-status diff-status--${file.status.toLowerCase()}`}
                >
                  {statusLabel(file.status)}
                </span>
                <code className="diff-file-path">{file.path}</code>
                <span className="diff-file-counts">
                  <span className="diff-add">+{file.additions}</span>
                  <span className="diff-del">−{file.deletions}</span>
                </span>
                {file.omitted && (
                  <span className="diff-omitted">
                    {file.omitted === "BINARY"
                      ? "binary"
                      : file.omitted === "TOO_LARGE"
                        ? "large"
                        : file.omitted === "NOT_A_FILE"
                          ? "not a file"
                          : file.omitted === "NON_UTF8"
                            ? "non-UTF-8"
                            : "budget"}
                  </span>
                )}
              </button>
            ))}
          </section>

          <div className="diff-viewer">
            {selected ? (
              <>
                <div className="diff-viewer-head">
                  <div>
                    <h4>
                      <code>{selected.path}</code>
                    </h4>
                    <p className="muted">
                      {statusLabel(selected.status)} · +{selected.additions} / −
                      {selected.deletions} · mode {selected.oldMode} →{" "}
                      {selected.newMode}
                    </p>
                  </div>
                  <div className="diff-viewer-actions">
                    <label className="diff-wrap-toggle">
                      <input
                        type="checkbox"
                        checked={wrap}
                        onChange={(event) => setWrap(event.target.checked)}
                      />
                      <WrapText size={14} aria-hidden="true" /> Wrap lines
                    </label>
                    {selected.patch && (
                      <CopyButton
                        label={`Copy patch for ${selected.path}`}
                        value={selected.patch}
                      />
                    )}
                  </div>
                </div>
                {selected.patch ? (
                  <>
                    {selected.patchTruncated && (
                      <p className="warning" role="status">
                        Patch truncated at Core's byte bound. Only the shown
                        lines are rendered.
                      </p>
                    )}
                    <DiffCode file={selected} wrap={wrap} />
                  </>
                ) : (
                  <div className="diff-omitted-panel" role="status">
                    <strong>No text patch.</strong>{" "}
                    {omissionLabel(selected) ??
                      "Core returned no patch for this file."}
                  </div>
                )}
              </>
            ) : (
              <EmptyState
                title="Select a changed file"
                description="Choose a file from the changed-file list to read its bounded unified patch."
              />
            )}
          </div>
        </div>
      )}

      <p className="muted diff-provenance">
        {data.source}
        {omittedFiles > 0
          ? ` · ${omittedFiles} file${omittedFiles === 1 ? "" : "s"} omitted a patch body`
          : ""}
      </p>
    </div>
  );
}
