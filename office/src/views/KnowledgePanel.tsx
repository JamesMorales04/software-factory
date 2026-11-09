import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Input, Textarea } from "../components/ui/Field";
import { LoadingState, Panel } from "../components/ui/Surface";
import {
  type AgenticKnowledgeResponse,
  type AgenticKnowledgeSource,
  type AgenticSkillRecord,
  CoreApiError,
  coreApi,
} from "../coreApi";
import { shortId } from "../format";
import { useI18n } from "../i18n";
import "./knowledge.css";

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function skillTone(skill: AgenticSkillRecord) {
  switch (skill.status) {
    case "ACTIVE":
      return "success" as const;
    case "PROMOTION_READY":
      return "attention" as const;
    case "REVOKED":
    case "EXPIRED":
      return "danger" as const;
    default:
      return "neutral" as const;
  }
}

function Sources({ sources }: { sources: AgenticKnowledgeSource[] }) {
  const { t } = useI18n();
  return (
    <ul className="knowledge-sources">
      {sources.map((source) => (
        <li key={`${source.url}:${source.title}`}>
          <a href={source.url} target="_blank" rel="noreferrer">
            {source.title}
          </a>
          <span className="muted">
            {source.kind}
            {source.reputation ? ` · ${source.reputation}` : ""}
            {source.benchmarkScore !== null &&
            source.benchmarkScore !== undefined
              ? ` · ${source.benchmarkScore}`
              : ""}
          </span>
          <p>{source.snippet}</p>
        </li>
      ))}
      <li className="muted">{t("knowledge.readOnly")}</li>
    </ul>
  );
}

export function KnowledgePanel({ requestId }: { requestId: string }) {
  const { t } = useI18n();
  const [data, setData] = useState<AgenticKnowledgeResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [library, setLibrary] = useState("");
  const [query, setQuery] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [license, setLicense] = useState("");
  const [evidence, setEvidence] = useState("");

  const load = useCallback(async () => {
    try {
      setData(await coreApi.getAgenticKnowledge(requestId));
      setError(null);
    } catch (cause) {
      setError(errorText(cause, t("knowledge.error.load")));
    }
  }, [requestId, t]);

  useEffect(() => {
    void load();
  }, [load]);

  const skill = data?.skills[0] ?? null;
  const adopted = skill
    ? (data?.adoptions.some((adoption) => adoption.skillId === skill.id) ??
      false)
    : false;

  async function research() {
    if (!library.trim() || !query.trim()) {
      setActionError(t("knowledge.gap.required"));
      return;
    }
    setBusy("research");
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.researchAgenticKnowledge(
        requestId,
        library.trim(),
        query.trim(),
      );
      await load();
      setNotice(t("knowledge.research.done"));
    } catch (cause) {
      setActionError(errorText(cause, t("knowledge.research.error")));
    } finally {
      setBusy(null);
    }
  }

  async function adopt() {
    if (!skill) return;
    setBusy("adopt");
    setActionError(null);
    try {
      await coreApi.adoptAgenticSkill(requestId, skill.id);
      await load();
      setNotice(t("knowledge.adopt.done"));
    } catch (cause) {
      setActionError(errorText(cause, t("knowledge.adopt.error")));
    } finally {
      setBusy(null);
    }
  }

  async function recordUse(outcome: "SUCCESS" | "FAILURE") {
    if (!skill) return;
    if (!evidence.trim()) {
      setActionError(t("knowledge.use.evidenceRequired"));
      return;
    }
    setBusy(outcome);
    setActionError(null);
    setNotice(null);
    try {
      await coreApi.recordAgenticSkillUse(skill.id, {
        request_id: requestId,
        outcome,
        evidence: evidence.trim(),
      });
      await load();
      setEvidence("");
      setNotice(
        outcome === "SUCCESS"
          ? t("knowledge.use.success")
          : t("knowledge.use.failure"),
      );
    } catch (cause) {
      setActionError(errorText(cause, t("knowledge.use.error")));
    } finally {
      setBusy(null);
    }
  }

  async function reviewLicense() {
    if (!skill) return;
    if (!license.trim()) {
      setActionError(t("knowledge.license.required"));
      return;
    }
    setBusy("license");
    setActionError(null);
    try {
      await coreApi.reviewAgenticSkillLicense(skill.id, license.trim());
      await load();
      setNotice(t("knowledge.license.done"));
    } catch (cause) {
      setActionError(errorText(cause, t("knowledge.license.error")));
    } finally {
      setBusy(null);
    }
  }

  if (error) {
    return (
      <Panel>
        <p className="ui-callout ui-callout--danger" role="alert">
          {error}
        </p>
        <Button variant="secondary" onClick={() => void load()}>
          {t("knowledge.retry")}
        </Button>
      </Panel>
    );
  }
  if (!data) {
    return (
      <Panel>
        <LoadingState label={t("knowledge.loading")} />
      </Panel>
    );
  }

  return (
    <Panel>
      <div className="section-heading">
        <h2>{t("knowledge.title")}</h2>
        {skill && <Badge tone={skillTone(skill)}>{skill.status}</Badge>}
      </div>
      <p className="muted">{t("knowledge.description")}</p>

      {actionError && (
        <p className="ui-callout ui-callout--danger" role="alert">
          {actionError}
        </p>
      )}
      {notice && (
        <p className="notice" role="status">
          {notice}
        </p>
      )}

      {data.inventory && (
        <div className="knowledge-inventory">
          <h3>{t("knowledge.explorer.title")}</h3>
          <p className="muted">
            {t("knowledge.explorer.manifests", {
              manifests: data.inventory.manifests.length
                ? data.inventory.manifests.join(", ")
                : t("knowledge.explorer.none"),
            })}
          </p>
          <p className="muted">
            {t("knowledge.explorer.libraries", {
              libraries: data.inventory.libraries.length
                ? data.inventory.libraries
                    .slice(0, 12)
                    .map((entry) => entry.name)
                    .join(", ")
                : t("knowledge.explorer.none"),
            })}
          </p>
        </div>
      )}

      {data.pack && skill ? (
        <>
          <div className="knowledge-pack">
            <div>
              <h3>{data.pack.library}</h3>
              <p className="muted">
                {data.pack.libraryId} ·{" "}
                {t("knowledge.pack.digest", {
                  digest: shortId(data.pack.packSha256, 16),
                })}
              </p>
            </div>
            <a href={data.pack.sourceUrl} target="_blank" rel="noreferrer">
              {t("knowledge.pack.openSource")}
            </a>
          </div>
          <Sources
            sources={
              JSON.parse(data.pack.packJson).sources as AgenticKnowledgeSource[]
            }
          />
          <dl className="knowledge-facts">
            <div>
              <dt>{t("knowledge.skill.scope")}</dt>
              <dd>{skill.scope}</dd>
            </div>
            <div>
              <dt>{t("knowledge.skill.status")}</dt>
              <dd>{skill.status}</dd>
            </div>
            <div>
              <dt>{t("knowledge.skill.uses")}</dt>
              <dd>
                {skill.successCount} / {skill.failureCount}
              </dd>
            </div>
            <div>
              <dt>{t("knowledge.skill.expires")}</dt>
              <dd>{skill.expiresAt}</dd>
            </div>
          </dl>
          {adopted ? (
            <p className="ui-callout ui-callout--success" role="status">
              {t("knowledge.adopt.active")}
            </p>
          ) : (
            <Button
              size="sm"
              busy={busy === "adopt"}
              disabled={busy !== null}
              onClick={() => void adopt()}
            >
              {t("knowledge.adopt.action")}
            </Button>
          )}
          <div className="knowledge-actions">
            <Textarea
              label={t("knowledge.use.evidence")}
              value={evidence}
              onChange={(event) => setEvidence(event.target.value)}
              rows={2}
            />
            <Button
              variant="secondary"
              size="sm"
              busy={busy === "SUCCESS"}
              disabled={busy !== null}
              onClick={() => void recordUse("SUCCESS")}
            >
              {t("knowledge.use.successAction")}
            </Button>
            <Button
              variant="secondary"
              size="sm"
              busy={busy === "FAILURE"}
              disabled={busy !== null}
              onClick={() => void recordUse("FAILURE")}
            >
              {t("knowledge.use.failureAction")}
            </Button>
          </div>
          <div className="knowledge-actions">
            <Input
              label={t("knowledge.license.label")}
              value={license}
              onChange={(event) => setLicense(event.target.value)}
              placeholder="MIT"
            />
            <Button
              variant="secondary"
              size="sm"
              busy={busy === "license"}
              disabled={busy !== null}
              onClick={() => void reviewLicense()}
            >
              {t("knowledge.license.action")}
            </Button>
            <Link className="button button--secondary button--md" to="/skills">
              {t("knowledge.openRegistry")}
            </Link>
          </div>
        </>
      ) : (
        <div className="knowledge-gap">
          <p className="muted">{t("knowledge.gap.description")}</p>
          <div className="knowledge-actions">
            <Input
              label={t("knowledge.gap.library")}
              value={library}
              onChange={(event) => setLibrary(event.target.value)}
              placeholder="chrono"
            />
            <Textarea
              label={t("knowledge.gap.query")}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              rows={2}
              placeholder={t("knowledge.gap.queryPlaceholder")}
            />
            <Button
              size="sm"
              busy={busy === "research"}
              disabled={busy !== null}
              onClick={() => void research()}
            >
              {t("knowledge.gap.action")}
            </Button>
          </div>
        </div>
      )}
    </Panel>
  );
}
