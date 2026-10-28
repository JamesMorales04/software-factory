import { ExternalLink, GitPullRequest, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import {
  type CandidatePullRequestCommentsResult,
  type CandidatePullRequestDiffResult,
  type CandidatePullRequestStatus,
  CoreApiError,
  coreApi,
  type ProjectGitHubBinding,
} from "../../coreApi";
import { formatTimestamp } from "../../format";
import { useI18n } from "../../i18n";
import { Button } from "../ui/Button";
import { Checkbox, Input, Select, Textarea } from "../ui/Field";
import { EmptyState, LoadingState, Panel } from "../ui/Surface";
import { StatusBadge } from "./StatusBadge";

function errorDetail(
  cause: unknown,
  fallback: string,
): {
  message: string;
  kind: string | null;
  blockers: string[];
} {
  if (!(cause instanceof CoreApiError)) {
    return { message: fallback, kind: null, blockers: [] };
  }
  const body =
    typeof cause.body === "object" && cause.body !== null
      ? (cause.body as Record<string, unknown>)
      : null;
  const blockers = Array.isArray(body?.blockers)
    ? body.blockers.filter((item): item is string => typeof item === "string")
    : [];
  return {
    message: cause.message,
    kind: body && typeof body.kind === "string" ? body.kind : null,
    blockers,
  };
}

function PullRequestChecks({ status }: { status: CandidatePullRequestStatus }) {
  const { t } = useI18n();
  const checks = status.checks;
  return (
    <div className="pr-checks" data-testid="pr-checks">
      <div className="pr-section-head">
        <h4>{t("pr.checks.title")}</h4>
        <StatusBadge status={checks.state} />
      </div>
      <p className="muted">
        {checks.detail} · head <code>{checks.headSha.slice(0, 12)}</code> ·{" "}
        {t("pr.checks.fetched", {
          date: formatTimestamp(checks.fetchedAt),
        })}
      </p>
      {checks.checkRuns.length === 0 && checks.statuses.length === 0 ? (
        <p className="muted">{t("pr.checks.empty")}</p>
      ) : (
        <div className="pr-check-table-wrap">
          <table className="pr-check-table">
            <thead>
              <tr>
                <th>{t("pr.checks.table.check")}</th>
                <th>{t("pr.checks.table.status")}</th>
                <th>{t("pr.checks.table.conclusion")}</th>
                <th>{t("pr.checks.table.source")}</th>
              </tr>
            </thead>
            <tbody>
              {checks.checkRuns.map((run) => (
                <tr key={`run-${run.id}`}>
                  <td>
                    {run.detailsUrl ? (
                      <a href={run.detailsUrl} target="_blank" rel="noreferrer">
                        {run.name}
                      </a>
                    ) : (
                      run.name
                    )}
                  </td>
                  <td>{run.status}</td>
                  <td>
                    <StatusBadge status={run.conclusion.toUpperCase()} />
                  </td>
                  <td className="muted">
                    {run.app ?? t("pr.checks.checkRun")}
                  </td>
                </tr>
              ))}
              {checks.statuses.map((entry) => (
                <tr key={`status-${entry.context}`}>
                  <td>{entry.context}</td>
                  <td>{t("pr.checks.commitStatus")}</td>
                  <td>
                    <StatusBadge status={entry.state.toUpperCase()} />
                  </td>
                  <td className="muted">
                    {entry.description || t("pr.checks.status")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {(checks.checkRunsTruncated || checks.statusesTruncated) && (
        <p className="warning">{t("pr.checks.truncated")}</p>
      )}
    </div>
  );
}

function PullRequestDiff({
  diff,
}: {
  diff: CandidatePullRequestDiffResult | null;
}) {
  const { t } = useI18n();
  if (!diff) return null;
  return (
    <div className="pr-diff" data-testid="pr-diff">
      <div className="pr-section-head">
        <h4>{t("pr.diff.title")}</h4>
        <span className="muted">
          {t("pr.diff.meta", {
            bytes: diff.diff.bytes.toLocaleString(),
            date: formatTimestamp(diff.diff.fetchedAt),
          })}
        </span>
      </div>
      {!diff.head_matches_candidate && (
        <p className="warning" role="status">
          {t("pr.diff.headMismatch")}
        </p>
      )}
      {diff.diff.truncated && (
        <p className="warning" role="status">
          {t("pr.diff.truncated")}
        </p>
      )}
      <section
        className="diff-code-shell diff-code-shell--wrap"
        aria-label={t("pr.diff.aria", { number: diff.pull_request.number })}
        // biome-ignore lint/a11y/noNoninteractiveTabindex: a scrollable diff region must be focusable so its overflow can be scrolled from the keyboard.
        tabIndex={0}
      >
        <pre className="diff-code diff-code--wrap">
          <code>{diff.diff.patch}</code>
        </pre>
      </section>
    </div>
  );
}

export function CandidatePullRequest({
  campaignId,
  candidateId,
  projectId,
  candidateCommitSha,
}: {
  campaignId: string;
  candidateId: string;
  projectId: string;
  candidateCommitSha: string;
}) {
  const { t } = useI18n();
  const [binding, setBinding] = useState<ProjectGitHubBinding | null>(null);
  const [bindingState, setBindingState] = useState<
    "loading" | "bound" | "unbound" | "error"
  >("loading");
  const [bindingError, setBindingError] = useState<string | null>(null);
  const [numberInput, setNumberInput] = useState("");
  const [status, setStatus] = useState<CandidatePullRequestStatus | null>(null);
  const [statusBusy, setStatusBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [errorKind, setErrorKind] = useState<string | null>(null);
  const [blockers, setBlockers] = useState<string[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [comments, setComments] =
    useState<CandidatePullRequestCommentsResult | null>(null);
  const [commentsBusy, setCommentsBusy] = useState(false);
  const [commentBody, setCommentBody] = useState("");
  const [commentBusy, setCommentBusy] = useState(false);
  const [reviewers, setReviewers] = useState("");
  const [reviewBusy, setReviewBusy] = useState(false);
  const [diff, setDiff] = useState<CandidatePullRequestDiffResult | null>(null);
  const [diffBusy, setDiffBusy] = useState(false);
  const [mergeConfirmed, setMergeConfirmed] = useState(false);
  const [mergeMethod, setMergeMethod] = useState<"merge" | "squash" | "rebase">(
    "squash",
  );
  const [mergeBusy, setMergeBusy] = useState(false);
  const [openHead, setOpenHead] = useState("");
  const [openTitle, setOpenTitle] = useState("");
  const [openBody, setOpenBody] = useState("");
  const [openDraft, setOpenDraft] = useState(false);
  const [openBusy, setOpenBusy] = useState(false);

  const loadBinding = useCallback(async () => {
    setBindingState("loading");
    setBindingError(null);
    try {
      const result = await coreApi.getProjectGitHub(projectId);
      setBinding(result.binding);
      setBindingState(result.binding ? "bound" : "unbound");
    } catch (cause) {
      setBinding(null);
      setBindingState("error");
      setBindingError(
        cause instanceof CoreApiError ? cause.message : t("pr.error.binding"),
      );
    }
  }, [projectId, t]);

  useEffect(() => {
    void loadBinding();
  }, [loadBinding]);

  const loadComments = useCallback(
    async (number: number) => {
      setCommentsBusy(true);
      try {
        setComments(
          await coreApi.getCandidatePullRequestComments(
            campaignId,
            candidateId,
            number,
          ),
        );
      } catch (cause) {
        const detail = errorDetail(cause, t("pr.error.comments"));
        setError(detail.message);
        setErrorKind(detail.kind);
      } finally {
        setCommentsBusy(false);
      }
    },
    [campaignId, candidateId, t],
  );

  async function loadStatus(number: number) {
    setStatusBusy(true);
    setError(null);
    setErrorKind(null);
    setBlockers([]);
    setNotice(null);
    setDiff(null);
    setComments(null);
    setMergeConfirmed(false);
    try {
      const next = await coreApi.getCandidatePullRequest(
        campaignId,
        candidateId,
        number,
      );
      setStatus(next);
      setBlockerList(next.merge_blocked_reasons);
      void loadComments(number);
    } catch (cause) {
      const detail = errorDetail(cause, t("pr.error.read"));
      setStatus(null);
      setError(detail.message);
      setErrorKind(detail.kind);
      setBlockerList(detail.blockers);
    } finally {
      setStatusBusy(false);
    }
  }

  function setBlockerList(values: string[]) {
    setBlockers(values);
  }

  async function loadDiff() {
    if (!status) return;
    setDiffBusy(true);
    setError(null);
    setErrorKind(null);
    try {
      setDiff(
        await coreApi.getCandidatePullRequestDiff(
          campaignId,
          candidateId,
          status.pull_request.number,
        ),
      );
    } catch (cause) {
      const detail = errorDetail(cause, t("pr.error.diff"));
      setError(detail.message);
      setErrorKind(detail.kind);
    } finally {
      setDiffBusy(false);
    }
  }

  async function submitComment() {
    if (!status || commentBody.trim() === "") return;
    setCommentBusy(true);
    setError(null);
    setErrorKind(null);
    try {
      await coreApi.commentOnCandidatePullRequest(
        campaignId,
        candidateId,
        status.pull_request.number,
        commentBody,
      );
      setCommentBody("");
      await loadStatus(status.pull_request.number);
      setNotice(t("pr.notice.comment"));
    } catch (cause) {
      const detail = errorDetail(cause, t("pr.error.comment"));
      setError(detail.message);
      setErrorKind(detail.kind);
    } finally {
      setCommentBusy(false);
    }
  }

  async function submitReview() {
    if (!status) return;
    const list = reviewers
      .split(",")
      .map((reviewer) => reviewer.trim())
      .filter(Boolean);
    if (list.length === 0) {
      setError(t("pr.review.empty"));
      setErrorKind("INVALID_REQUEST");
      return;
    }
    setReviewBusy(true);
    setError(null);
    setErrorKind(null);
    try {
      const result = await coreApi.requestCandidatePullRequestReview(
        campaignId,
        candidateId,
        status.pull_request.number,
        list,
      );
      setNotice(
        t("pr.notice.review", { reviewers: result.reviewers.join(", ") }),
      );
      setReviewers("");
    } catch (cause) {
      const detail = errorDetail(cause, t("pr.error.review"));
      setError(detail.message);
      setErrorKind(detail.kind);
    } finally {
      setReviewBusy(false);
    }
  }

  async function submitMerge() {
    if (!status || !mergeConfirmed) return;
    setMergeBusy(true);
    setError(null);
    setErrorKind(null);
    setBlockers([]);
    try {
      const result = await coreApi.mergeCandidatePullRequest(
        campaignId,
        candidateId,
        {
          number: status.pull_request.number,
          expected_head_sha: status.pull_request.headSha,
          merge_method: mergeMethod,
          confirmed: true,
        },
      );
      setMergeConfirmed(false);
      await loadStatus(status.pull_request.number);
      setNotice(
        t("pr.notice.merge", {
          sha: result.merge_commit_sha.slice(0, 12) || t("pr.merge.noSha"),
          message: result.message,
        }),
      );
    } catch (cause) {
      const detail = errorDetail(cause, t("pr.error.merge"));
      await loadStatus(status.pull_request.number);
      setError(detail.message);
      setErrorKind(detail.kind);
      setBlockerList(detail.blockers);
    } finally {
      setMergeBusy(false);
    }
  }

  async function submitOpen() {
    if (!binding) return;
    setOpenBusy(true);
    setError(null);
    setErrorKind(null);
    setNotice(null);
    try {
      const result = await coreApi.openCandidatePullRequest(
        campaignId,
        candidateId,
        {
          title: openTitle,
          body: openBody,
          head: openHead,
          draft: openDraft,
        },
      );
      setNumberInput(String(result.pull_request.number));
      await loadStatus(result.pull_request.number);
      setNotice(t("pr.notice.open", { number: result.pull_request.number }));
    } catch (cause) {
      const detail = errorDetail(cause, t("pr.error.open"));
      setError(detail.message);
      setErrorKind(detail.kind);
    } finally {
      setOpenBusy(false);
    }
  }

  return (
    <Panel className="candidate-pr" data-testid="candidate-pr">
      <div className="pr-heading">
        <div>
          <h3>
            <GitPullRequest size={17} aria-hidden="true" /> {t("pr.title")}
          </h3>
          <p className="muted">{t("pr.description")}</p>
        </div>
        {binding && (
          <div className="pr-binding">
            <span>
              <strong>{binding.repo_full_name}</strong>
            </span>
            <span className="muted">
              {t("pr.binding.base", { branch: binding.default_branch })}
            </span>
            <Button
              size="sm"
              variant="quiet"
              onClick={() => void loadBinding()}
              aria-label={t("pr.binding.refreshAria")}
            >
              <RefreshCw size={14} aria-hidden="true" />{" "}
              {t("pr.binding.refresh")}
            </Button>
          </div>
        )}
      </div>

      {bindingState === "loading" && (
        <LoadingState label={t("pr.binding.loading")} />
      )}
      {bindingState === "unbound" && (
        <div className="ui-callout ui-callout--attention" role="status">
          {t("pr.binding.unbound")}
        </div>
      )}
      {bindingState === "error" && (
        <div className="ui-callout ui-callout--danger" role="alert">
          {bindingError}
        </div>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}
      {error && (
        <div className="ui-callout ui-callout--danger" role="alert">
          <strong>{errorKind ?? t("pr.error.title")}.</strong> {error}
        </div>
      )}
      {blockers.length > 0 && (
        <div className="ui-callout ui-callout--attention" role="status">
          <strong>{t("pr.merge.blockedTitle")}</strong>
          <ul>
            {blockers.map((blocker) => (
              <li key={blocker}>{blocker}</li>
            ))}
          </ul>
        </div>
      )}

      {bindingState === "bound" && binding && (
        <>
          <div className="pr-lookup">
            <Input
              label={t("pr.lookup.label")}
              inputMode="numeric"
              value={numberInput}
              onChange={(event) => setNumberInput(event.target.value)}
              placeholder="42"
              help={t("pr.lookup.help")}
            />
            <Button
              variant="primary"
              disabled={statusBusy || !/^[1-9][0-9]*$/.test(numberInput.trim())}
              onClick={() => void loadStatus(Number(numberInput.trim()))}
            >
              {statusBusy ? t("pr.lookup.reading") : t("pr.lookup.read")}
            </Button>
          </div>

          {status && (
            <div className="pr-status" data-testid="pr-status">
              <div className="pr-section-head">
                <h4>
                  <a
                    href={status.pull_request.htmlUrl}
                    target="_blank"
                    rel="noreferrer"
                  >
                    #{status.pull_request.number} {status.pull_request.title}{" "}
                    <ExternalLink size={13} aria-hidden="true" />
                  </a>
                </h4>
                <div className="pr-badges">
                  <StatusBadge
                    status={status.pull_request.state.toUpperCase()}
                  />
                  {status.pull_request.draft && (
                    <StatusBadge status="DRAFT" label="DRAFT" />
                  )}
                  <StatusBadge
                    status={status.pull_request.merged ? "MERGED" : "OPEN"}
                  />
                  <StatusBadge
                    status={
                      status.merge_ready
                        ? "READY"
                        : status.checks.state === "FAILURE"
                          ? "FAILURE"
                          : "BLOCKED"
                    }
                  />
                </div>
              </div>
              <dl className="detail pr-facts">
                <dt>{t("pr.facts.head")}</dt>
                <dd>
                  <code>{status.pull_request.headRef}</code>{" "}
                  <code title={status.pull_request.headSha}>
                    {status.pull_request.headSha.slice(0, 12)}
                  </code>{" "}
                  <span
                    className={
                      status.head_matches_candidate ? "diff-add" : "warning"
                    }
                  >
                    {status.head_matches_candidate
                      ? t("pr.facts.headMatches")
                      : t("pr.facts.headMismatch")}
                  </span>
                </dd>
                <dt>{t("pr.facts.base")}</dt>
                <dd>
                  <code>{status.pull_request.baseRef}</code>{" "}
                  <code title={status.pull_request.baseSha}>
                    {status.pull_request.baseSha.slice(0, 12)}
                  </code>
                </dd>
                <dt>{t("pr.facts.mergeable")}</dt>
                <dd>
                  {status.pull_request.mergeable === null
                    ? t("pr.facts.mergeableUnknown")
                    : status.pull_request.mergeable
                      ? t("pr.facts.mergeableYes")
                      : t("pr.facts.mergeableNo")}{" "}
                  ·{" "}
                  {t("pr.facts.mergeableState", {
                    state: status.pull_request.mergeableState,
                  })}
                </dd>
                <dt>{t("pr.facts.changes")}</dt>
                <dd>
                  {t("pr.facts.changesValue", {
                    files: status.pull_request.changedFiles,
                    additions: status.pull_request.additions,
                    deletions: status.pull_request.deletions,
                  })}
                </dd>
                <dt>{t("pr.facts.gate")}</dt>
                <dd>{status.gate?.outcome ?? t("pr.facts.gateNone")}</dd>
                <dt>{t("pr.facts.checked")}</dt>
                <dd>{formatTimestamp(status.checked_at)}</dd>
              </dl>

              <PullRequestChecks status={status} />

              <div className="pr-actions">
                <Button
                  size="sm"
                  onClick={() => void loadStatus(status.pull_request.number)}
                  disabled={statusBusy}
                >
                  <RefreshCw size={14} aria-hidden="true" />{" "}
                  {t("pr.actions.freshRead")}
                </Button>
                <Button
                  size="sm"
                  onClick={() => void loadDiff()}
                  disabled={diffBusy}
                >
                  {diffBusy ? t("pr.diff.reading") : t("pr.diff.read")}
                </Button>
              </div>

              <PullRequestDiff diff={diff} />

              <div className="pr-grid">
                <section aria-labelledby="pr-comments-heading">
                  <div className="pr-section-head">
                    <h4 id="pr-comments-heading">{t("pr.comments.title")}</h4>
                    <span className="muted">
                      {t("pr.comments.shown", {
                        count: comments?.comments.comments.length ?? 0,
                      })}
                    </span>
                  </div>
                  {commentsBusy ? (
                    <LoadingState label={t("pr.comments.loading")} />
                  ) : !comments || comments.comments.comments.length === 0 ? (
                    <EmptyState
                      title={t("pr.comments.emptyTitle")}
                      description={t("pr.comments.emptyDescription")}
                    />
                  ) : (
                    <ul className="pr-comments">
                      {comments.comments.comments.map((comment) => (
                        <li key={`${comment.kind}-${comment.id}`}>
                          <div className="pr-comment-head">
                            <strong>
                              {comment.author ?? t("pr.comments.unknownAuthor")}
                            </strong>
                            <span className="muted">
                              {comment.kind} ·{" "}
                              {formatTimestamp(comment.createdAt)}
                              {comment.path
                                ? ` · ${comment.path}${comment.line ? `:${comment.line}` : ""}`
                                : ""}
                            </span>
                          </div>
                          <p className="pr-comment-body">{comment.body}</p>
                          {comment.bodyTruncated && (
                            <span className="warning">
                              {t("pr.comments.truncatedBody")}
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                  {comments?.comments.truncated && (
                    <p className="warning">{t("pr.comments.listTruncated")}</p>
                  )}
                  <Textarea
                    label={t("pr.comments.addLabel")}
                    value={commentBody}
                    onChange={(event) => setCommentBody(event.target.value)}
                    placeholder={t("pr.comments.placeholder")}
                    rows={3}
                  />
                  <Button
                    size="sm"
                    onClick={() => void submitComment()}
                    disabled={commentBusy || commentBody.trim() === ""}
                  >
                    {commentBusy
                      ? t("pr.comments.posting")
                      : t("pr.comments.post")}
                  </Button>
                </section>

                <section aria-labelledby="pr-review-heading">
                  <div className="pr-section-head">
                    <h4 id="pr-review-heading">{t("pr.review.title")}</h4>
                  </div>
                  <Input
                    label={t("pr.review.logins")}
                    value={reviewers}
                    onChange={(event) => setReviewers(event.target.value)}
                    placeholder="octocat, reviewer"
                    help={t("pr.review.help")}
                  />
                  <Button
                    size="sm"
                    onClick={() => void submitReview()}
                    disabled={reviewBusy || reviewers.trim() === ""}
                  >
                    {reviewBusy
                      ? t("pr.review.requesting")
                      : t("pr.review.request")}
                  </Button>
                </section>
              </div>

              <section className="pr-merge" aria-labelledby="pr-merge-heading">
                <div className="pr-section-head">
                  <h4 id="pr-merge-heading">{t("pr.merge.title")}</h4>
                  <span className="muted">{t("pr.merge.hint")}</span>
                </div>
                <p className="muted">
                  {t("pr.merge.expectedHead")}{" "}
                  <code title={candidateCommitSha}>
                    {candidateCommitSha.slice(0, 12)}
                  </code>{" "}
                  {t("pr.merge.expectedHeadDetail")}
                </p>
                <Select
                  label={t("pr.merge.method")}
                  value={mergeMethod}
                  onChange={(event) =>
                    setMergeMethod(
                      event.target.value as "merge" | "squash" | "rebase",
                    )
                  }
                >
                  <option value="squash">{t("pr.merge.squash")}</option>
                  <option value="merge">{t("pr.merge.mergeCommit")}</option>
                  <option value="rebase">{t("pr.merge.rebase")}</option>
                </Select>
                <Checkbox
                  label={t("pr.merge.confirm")}
                  checked={mergeConfirmed}
                  onChange={(event) => setMergeConfirmed(event.target.checked)}
                />
                <Button
                  variant="danger"
                  onClick={() => void submitMerge()}
                  disabled={
                    !mergeConfirmed ||
                    !status.merge_ready ||
                    statusBusy ||
                    mergeBusy
                  }
                >
                  {mergeBusy ? t("pr.merge.merging") : t("pr.merge.submit")}
                </Button>
                {!status.merge_ready && (
                  <p className="warning">{t("pr.merge.disabled")}</p>
                )}
              </section>
            </div>
          )}

          <details className="pr-open">
            <summary>{t("pr.open.summary")}</summary>
            <p className="muted">
              {t("pr.open.hint")}{" "}
              <code title={candidateCommitSha}>
                {candidateCommitSha.slice(0, 12)}
              </code>
              . {t("pr.open.hintSuffix")}
            </p>
            <Input
              label={t("pr.open.head")}
              value={openHead}
              onChange={(event) => setOpenHead(event.target.value)}
              placeholder="campaign/candidate-42"
            />
            <Input
              label={t("pr.open.title")}
              value={openTitle}
              onChange={(event) => setOpenTitle(event.target.value)}
              maxLength={256}
            />
            <Textarea
              label={t("pr.open.body")}
              value={openBody}
              onChange={(event) => setOpenBody(event.target.value)}
              rows={3}
            />
            <Checkbox
              label={t("pr.open.draft")}
              checked={openDraft}
              onChange={(event) => setOpenDraft(event.target.checked)}
            />
            <Button
              size="sm"
              onClick={() => void submitOpen()}
              disabled={
                openBusy || openHead.trim() === "" || openTitle.trim() === ""
              }
            >
              {openBusy ? t("pr.open.opening") : t("pr.open.submit")}
            </Button>
          </details>
        </>
      )}
    </Panel>
  );
}
