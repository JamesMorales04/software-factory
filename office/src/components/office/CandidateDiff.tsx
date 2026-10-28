import { Check, Copy, FileDiff, WrapText } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import {
  type CandidateDiffSummary,
  type CandidateFileDiff,
  CoreApiError,
  coreApi,
} from "../../coreApi";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { EmptyState, LoadingState } from "../ui/Surface";

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KiB`;
  return `${(value / (1024 * 1024)).toFixed(1)} MiB`;
}

function omissionLabel(
  file: CandidateFileDiff,
  t: ReturnType<typeof useI18n>["t"],
): string | null {
  switch (file.omitted) {
    case "BINARY":
      return t("diff.omitted.binaryDescription");
    case "TOO_LARGE":
      return t("diff.omitted.tooLargeDescription");
    case "NOT_A_FILE":
      return t("diff.omitted.notAFileDescription");
    case "NON_UTF8":
      return t("diff.omitted.nonUtf8Description");
    case "BUDGET":
      return t("diff.omitted.budgetDescription");
    default:
      return null;
  }
}

function statusLabel(
  status: CandidateFileDiff["status"],
  t: ReturnType<typeof useI18n>["t"],
): string {
  if (status === "ADDED") return t("diff.status.added");
  if (status === "DELETED") return t("diff.status.deleted");
  return t("diff.status.modified");
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
  const { t } = useI18n();
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
        {state === "copied" ? t("diff.copy.copied") : t("diff.copy.copy")}
      </Button>
      <span className="muted" role="status" aria-live="polite">
        {state === "unavailable" ? t("diff.copy.unavailable") : ""}
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
  const { t } = useI18n();
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
      aria-label={t("diff.unifiedAria", { path: file.path })}
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
  const { t } = useI18n();
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
        setError(t("diff.error.fallback"));
      }
    } finally {
      setBusy(false);
    }
  }, [campaignId, candidateId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const selected =
    data?.files.find((file) => file.path === selectedPath) ?? null;

  if (busy && !data) return <LoadingState label={t("diff.loading")} />;
  if (error) {
    return (
      <div className="ui-callout ui-callout--danger" role="alert">
        <strong>{t("diff.error.title")}</strong> {error}
        {errorKind && <span className="muted"> ({errorKind})</span>}{" "}
        <Button size="sm" onClick={() => void load()}>
          {t("common.retry")}
        </Button>
      </div>
    );
  }
  if (!data) return <LoadingState label={t("diff.loading")} />;

  const omittedFiles = data.files.filter((file) => file.omitted).length;
  const gateLabel = data.gate ? data.gate.outcome : t("diff.gate.noneRecorded");

  return (
    <div className="candidate-diff" data-testid="candidate-diff">
      <div className="diff-heading">
        <div>
          <h3>
            <FileDiff size={17} aria-hidden="true" /> {t("diff.title")}
          </h3>
          <p className="muted">{t("diff.description")}</p>
        </div>
        <section className="diff-totals" aria-label={t("diff.totals.aria")}>
          <span>
            <strong>{data.returned_files}</strong>{" "}
            {data.files_truncated
              ? t("diff.totals.shown")
              : data.returned_files === 1
                ? t("diff.totals.file")
                : t("diff.totals.files")}
          </span>
          <span className="diff-add">+{data.total_additions}</span>
          <span className="diff-del">−{data.total_deletions}</span>
          <span className="muted">
            {t("diff.totals.patch", {
              size: formatBytes(data.patch_bytes),
            })}
          </span>
        </section>
      </div>

      <dl className="diff-revisions">
        <RevisionFact
          label={t("diff.revision.base")}
          value={data.base_commit}
          copyLabel={t("diff.revision.copyBase")}
        />
        <RevisionFact
          label={t("diff.revision.candidate")}
          value={data.commit_sha}
          copyLabel={t("diff.revision.copyCandidate")}
        />
        <RevisionFact
          label={t("diff.revision.tree")}
          value={data.tree_sha}
          copyLabel={t("diff.revision.copyTree")}
        />
        <div className="diff-revision-fact">
          <dt>{t("diff.revision.candidateLabel")}</dt>
          <dd>
            <code title={data.candidate.id}>
              {data.candidate.id.slice(0, 12)}
            </code>{" "}
            <span className="muted">
              {t("diff.revision.implementer", {
                id: data.candidate.implementer_id || t("common.notRecorded"),
              })}
            </span>
          </dd>
        </div>
        <div className="diff-revision-fact">
          <dt>{t("diff.revision.gateIdentity")}</dt>
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
          {t("diff.truncated.files", { count: data.returned_files })}
        </div>
      )}
      {data.patch_truncated && (
        <div className="ui-callout ui-callout--attention" role="status">
          {t("diff.truncated.patch")}
        </div>
      )}

      <p className="diff-stat">
        <span className="muted">{t("diff.gitStat")} </span>
        <code>{data.diff_stat || t("diff.noDiffStat")}</code>
      </p>

      {data.files.length === 0 ? (
        <EmptyState
          title={t("diff.empty.title")}
          description={t("diff.empty.description")}
        />
      ) : (
        <div className="diff-workspace">
          <section
            className="diff-file-list"
            aria-label={t("diff.fileList.aria")}
          >
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
                  {statusLabel(file.status, t)}
                </span>
                <code className="diff-file-path">{file.path}</code>
                <span className="diff-file-counts">
                  <span className="diff-add">+{file.additions}</span>
                  <span className="diff-del">−{file.deletions}</span>
                </span>
                {file.omitted && (
                  <span className="diff-omitted">
                    {file.omitted === "BINARY"
                      ? t("diff.omitted.binary")
                      : file.omitted === "TOO_LARGE"
                        ? t("diff.omitted.large")
                        : file.omitted === "NOT_A_FILE"
                          ? t("diff.omitted.notAFile")
                          : file.omitted === "NON_UTF8"
                            ? t("diff.omitted.nonUtf8")
                            : t("diff.omitted.budget")}
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
                      {statusLabel(selected.status, t)} · +{selected.additions}{" "}
                      / −{selected.deletions} ·{" "}
                      {t("diff.viewer.mode", {
                        oldMode: selected.oldMode,
                        newMode: selected.newMode,
                      })}
                    </p>
                  </div>
                  <div className="diff-viewer-actions">
                    <label className="diff-wrap-toggle">
                      <input
                        type="checkbox"
                        checked={wrap}
                        onChange={(event) => setWrap(event.target.checked)}
                      />
                      <WrapText size={14} aria-hidden="true" />{" "}
                      {t("diff.viewer.wrap")}
                    </label>
                    {selected.patch && (
                      <CopyButton
                        label={t("diff.viewer.copyPatch", {
                          path: selected.path,
                        })}
                        value={selected.patch}
                      />
                    )}
                  </div>
                </div>
                {selected.patch ? (
                  <>
                    {selected.patchTruncated && (
                      <p className="warning" role="status">
                        {t("diff.viewer.patchTruncated")}
                      </p>
                    )}
                    <DiffCode file={selected} wrap={wrap} />
                  </>
                ) : (
                  <div className="diff-omitted-panel" role="status">
                    <strong>{t("diff.viewer.noPatchTitle")}</strong>{" "}
                    {omissionLabel(selected, t) ??
                      t("diff.viewer.noPatchDescription")}
                  </div>
                )}
              </>
            ) : (
              <EmptyState
                title={t("diff.select.title")}
                description={t("diff.select.description")}
              />
            )}
          </div>
        </div>
      )}

      <p className="muted diff-provenance">
        {data.source}
        {omittedFiles > 0
          ? ` · ${t("diff.provenance.omitted", { count: omittedFiles })}`
          : ""}
      </p>
    </div>
  );
}
