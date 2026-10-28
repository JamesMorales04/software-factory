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
  const checks = status.checks;
  return (
    <div className="pr-checks" data-testid="pr-checks">
      <div className="pr-section-head">
        <h4>Checks</h4>
        <StatusBadge status={checks.state} />
      </div>
      <p className="muted">
        {checks.detail} · head <code>{checks.headSha.slice(0, 12)}</code> ·
        fetched {formatTimestamp(checks.fetchedAt)}
      </p>
      {checks.checkRuns.length === 0 && checks.statuses.length === 0 ? (
        <p className="muted">
          GitHub reported no check run or commit status for this head. No
          success is inferred.
        </p>
      ) : (
        <div className="pr-check-table-wrap">
          <table className="pr-check-table">
            <thead>
              <tr>
                <th>Check</th>
                <th>Status</th>
                <th>Conclusion</th>
                <th>Source</th>
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
                  <td className="muted">{run.app ?? "check run"}</td>
                </tr>
              ))}
              {checks.statuses.map((entry) => (
                <tr key={`status-${entry.context}`}>
                  <td>{entry.context}</td>
                  <td>commit status</td>
                  <td>
                    <StatusBadge status={entry.state.toUpperCase()} />
                  </td>
                  <td className="muted">{entry.description || "status"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {(checks.checkRunsTruncated || checks.statusesTruncated) && (
        <p className="warning">
          Check results were truncated at Core's bound. Merge readiness stays
          blocked until a complete fresh read is available.
        </p>
      )}
    </div>
  );
}

function PullRequestDiff({
  diff,
}: {
  diff: CandidatePullRequestDiffResult | null;
}) {
  if (!diff) return null;
  return (
    <div className="pr-diff" data-testid="pr-diff">
      <div className="pr-section-head">
        <h4>Pull request diff</h4>
        <span className="muted">
          {diff.diff.bytes.toLocaleString()} bytes · fetched{" "}
          {formatTimestamp(diff.diff.fetchedAt)}
        </span>
      </div>
      {!diff.head_matches_candidate && (
        <p className="warning" role="status">
          This pull request head does not match the recorded Candidate commit.
          The diff is shown for the pull request, not as Candidate evidence.
        </p>
      )}
      {diff.diff.truncated && (
        <p className="warning" role="status">
          Pull request diff truncated at Core's byte bound.
        </p>
      )}
      <section
        className="diff-code-shell diff-code-shell--wrap"
        aria-label={`Unified diff for pull request ${diff.pull_request.number}`}
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
        cause instanceof CoreApiError
          ? cause.message
          : "Failed to read the Project's GitHub binding.",
      );
    }
  }, [projectId]);

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
        const detail = errorDetail(
          cause,
          "Failed to read pull request comments.",
        );
        setError(detail.message);
        setErrorKind(detail.kind);
      } finally {
        setCommentsBusy(false);
      }
    },
    [campaignId, candidateId],
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
      const detail = errorDetail(cause, "Failed to read the pull request.");
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
      const detail = errorDetail(
        cause,
        "Failed to read the pull request diff.",
      );
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
      setNotice("Comment accepted by GitHub through Core.");
    } catch (cause) {
      const detail = errorDetail(cause, "Failed to post the comment.");
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
      setError("Enter at least one GitHub reviewer login.");
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
        `Review requested from ${result.reviewers.join(", ")}. No approval is inferred.`,
      );
      setReviewers("");
    } catch (cause) {
      const detail = errorDetail(cause, "Failed to request review.");
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
        `Merged as ${result.merge_commit_sha.slice(0, 12) || "a merge commit recorded by GitHub"}. ${result.message}`,
      );
    } catch (cause) {
      const detail = errorDetail(cause, "Merge was not accepted.");
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
      setNotice(
        `Opened pull request #${result.pull_request.number}. Core verified the head branch resolved to the exact Candidate commit.`,
      );
    } catch (cause) {
      const detail = errorDetail(cause, "Pull request was not opened.");
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
            <GitPullRequest size={17} aria-hidden="true" /> GitHub pull request
          </h3>
          <p className="muted">
            Core uses the encrypted GitHub connection. Tokens never reach this
            page; branch protection and required checks remain
            GitHub-authoritative.
          </p>
        </div>
        {binding && (
          <div className="pr-binding">
            <span>
              <strong>{binding.repo_full_name}</strong>
            </span>
            <span className="muted">base {binding.default_branch}</span>
            <Button
              size="sm"
              variant="quiet"
              onClick={() => void loadBinding()}
              aria-label="Refresh Project GitHub binding"
            >
              <RefreshCw size={14} aria-hidden="true" /> Refresh
            </Button>
          </div>
        )}
      </div>

      {bindingState === "loading" && (
        <LoadingState label="Reading the Project GitHub binding…" />
      )}
      {bindingState === "unbound" && (
        <div className="ui-callout ui-callout--attention" role="status">
          This Project is not bound to a GitHub repository. Pull request reads
          and actions are unavailable until the Project is bound in Projects.
          Core does not infer a repository or PR.
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
          <strong>{errorKind ?? "Pull request operation failed"}.</strong>{" "}
          {error}
        </div>
      )}
      {blockers.length > 0 && (
        <div className="ui-callout ui-callout--attention" role="status">
          <strong>Merge blocked by Core's fresh read.</strong>
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
              label="Pull request number"
              inputMode="numeric"
              value={numberInput}
              onChange={(event) => setNumberInput(event.target.value)}
              placeholder="42"
              help="Reads an existing PR from GitHub. No PR is invented from a candidate."
            />
            <Button
              variant="primary"
              disabled={statusBusy || !/^[1-9][0-9]*$/.test(numberInput.trim())}
              onClick={() => void loadStatus(Number(numberInput.trim()))}
            >
              {statusBusy ? "Reading…" : "Read PR and checks"}
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
                <dt>Head</dt>
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
                      ? "matches Candidate"
                      : "does not match Candidate"}
                  </span>
                </dd>
                <dt>Base</dt>
                <dd>
                  <code>{status.pull_request.baseRef}</code>{" "}
                  <code title={status.pull_request.baseSha}>
                    {status.pull_request.baseSha.slice(0, 12)}
                  </code>
                </dd>
                <dt>GitHub mergeable</dt>
                <dd>
                  {status.pull_request.mergeable === null
                    ? "unknown (GitHub has not computed it)"
                    : status.pull_request.mergeable
                      ? "yes"
                      : "no"}{" "}
                  · state <code>{status.pull_request.mergeableState}</code>
                </dd>
                <dt>Changes</dt>
                <dd>
                  {status.pull_request.changedFiles} files · +
                  {status.pull_request.additions} / −
                  {status.pull_request.deletions}
                </dd>
                <dt>Gate</dt>
                <dd>{status.gate?.outcome ?? "no Gate decision recorded"}</dd>
                <dt>Checked</dt>
                <dd>{formatTimestamp(status.checked_at)}</dd>
              </dl>

              <PullRequestChecks status={status} />

              <div className="pr-actions">
                <Button
                  size="sm"
                  onClick={() => void loadStatus(status.pull_request.number)}
                  disabled={statusBusy}
                >
                  <RefreshCw size={14} aria-hidden="true" /> Fresh read
                </Button>
                <Button
                  size="sm"
                  onClick={() => void loadDiff()}
                  disabled={diffBusy}
                >
                  {diffBusy ? "Reading diff…" : "Read PR diff"}
                </Button>
              </div>

              <PullRequestDiff diff={diff} />

              <div className="pr-grid">
                <section aria-labelledby="pr-comments-heading">
                  <div className="pr-section-head">
                    <h4 id="pr-comments-heading">Comments</h4>
                    <span className="muted">
                      {comments?.comments.comments.length ?? 0} shown
                    </span>
                  </div>
                  {commentsBusy ? (
                    <LoadingState label="Reading comments…" />
                  ) : !comments || comments.comments.comments.length === 0 ? (
                    <EmptyState
                      title="No comments"
                      description="GitHub returned no issue or review comment for this pull request."
                    />
                  ) : (
                    <ul className="pr-comments">
                      {comments.comments.comments.map((comment) => (
                        <li key={`${comment.kind}-${comment.id}`}>
                          <div className="pr-comment-head">
                            <strong>
                              {comment.author ?? "unknown author"}
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
                              Comment body truncated by Core's bound.
                            </span>
                          )}
                        </li>
                      ))}
                    </ul>
                  )}
                  {comments?.comments.truncated && (
                    <p className="warning">
                      Comment list truncated at Core's bound.
                    </p>
                  )}
                  <Textarea
                    label="Add a comment"
                    value={commentBody}
                    onChange={(event) => setCommentBody(event.target.value)}
                    placeholder="Record a concrete finding or request."
                    rows={3}
                  />
                  <Button
                    size="sm"
                    onClick={() => void submitComment()}
                    disabled={commentBusy || commentBody.trim() === ""}
                  >
                    {commentBusy ? "Posting…" : "Post comment"}
                  </Button>
                </section>

                <section aria-labelledby="pr-review-heading">
                  <div className="pr-section-head">
                    <h4 id="pr-review-heading">Request review</h4>
                  </div>
                  <Input
                    label="Reviewer logins"
                    value={reviewers}
                    onChange={(event) => setReviewers(event.target.value)}
                    placeholder="octocat, reviewer"
                    help="Comma-separated GitHub logins. A request is not an approval."
                  />
                  <Button
                    size="sm"
                    onClick={() => void submitReview()}
                    disabled={reviewBusy || reviewers.trim() === ""}
                  >
                    {reviewBusy ? "Requesting…" : "Request review"}
                  </Button>
                </section>
              </div>

              <section className="pr-merge" aria-labelledby="pr-merge-heading">
                <div className="pr-section-head">
                  <h4 id="pr-merge-heading">Authorized merge</h4>
                  <span className="muted">
                    no auto-merge · Core re-reads PR and checks immediately
                    before merge
                  </span>
                </div>
                <p className="muted">
                  Expected head{" "}
                  <code title={candidateCommitSha}>
                    {candidateCommitSha.slice(0, 12)}
                  </code>{" "}
                  is pinned in the merge request. GitHub still enforces branch
                  protection and required checks. A blocked response is shown as
                  blocked, never as success.
                </p>
                <Select
                  label="Merge method"
                  value={mergeMethod}
                  onChange={(event) =>
                    setMergeMethod(
                      event.target.value as "merge" | "squash" | "rebase",
                    )
                  }
                >
                  <option value="squash">Squash</option>
                  <option value="merge">Merge commit</option>
                  <option value="rebase">Rebase</option>
                </Select>
                <Checkbox
                  label="I explicitly confirm this merge of the exact expected head."
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
                  {mergeBusy ? "Merging…" : "Merge pull request"}
                </Button>
                {!status.merge_ready && (
                  <p className="warning">
                    Merge is disabled until Core's fresh read reports a clean,
                    matching head with complete successful checks.
                  </p>
                )}
              </section>
            </div>
          )}

          <details className="pr-open">
            <summary>Open a pull request from an exact branch</summary>
            <p className="muted">
              Core verifies the GitHub branch resolves to{" "}
              <code title={candidateCommitSha}>
                {candidateCommitSha.slice(0, 12)}
              </code>
              . A mismatched or missing branch is a typed blocked result.
            </p>
            <Input
              label="Head branch"
              value={openHead}
              onChange={(event) => setOpenHead(event.target.value)}
              placeholder="campaign/candidate-42"
            />
            <Input
              label="Title"
              value={openTitle}
              onChange={(event) => setOpenTitle(event.target.value)}
              maxLength={256}
            />
            <Textarea
              label="Body"
              value={openBody}
              onChange={(event) => setOpenBody(event.target.value)}
              rows={3}
            />
            <Checkbox
              label="Open as draft"
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
              {openBusy ? "Opening…" : "Open pull request"}
            </Button>
          </details>
        </>
      )}
    </Panel>
  );
}
