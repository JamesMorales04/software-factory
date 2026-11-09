import { useCallback, useEffect, useState } from "react";
import { Badge } from "../components/ui/Badge";
import { Button } from "../components/ui/Button";
import { Checkbox, Input } from "../components/ui/Field";
import {
  EmptyState,
  ErrorState,
  LoadingState,
  PageHeader,
  Panel,
} from "../components/ui/Surface";
import {
  type AgenticSkillRecord,
  type AgenticSkillRegistryEntry,
  CoreApiError,
  coreApi,
} from "../coreApi";
import { useI18n } from "../i18n";
import "./knowledge.css";

function errorText(cause: unknown, fallback: string): string {
  return cause instanceof CoreApiError ? cause.message : fallback;
}

function scopeTone(scope: string) {
  switch (scope) {
    case "GLOBAL":
      return "danger" as const;
    case "PROJECT":
      return "success" as const;
    default:
      return "info" as const;
  }
}

function manifestSource(manifestJson: string): string | null {
  try {
    const manifest = JSON.parse(manifestJson) as { sourceUrl?: string };
    return manifest.sourceUrl ?? null;
  } catch {
    return null;
  }
}

export function Skills() {
  const { t, formatDate } = useI18n();
  const [entries, setEntries] = useState<AgenticSkillRegistryEntry[] | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [licenses, setLicenses] = useState<Record<string, string>>({});
  const [reasons, setReasons] = useState<Record<string, string>>({});
  const [ownerApproved, setOwnerApproved] = useState<Record<string, boolean>>(
    {},
  );

  const load = useCallback(async () => {
    try {
      const result = await coreApi.listAgenticSkills();
      setEntries(result.skills);
      setError(null);
    } catch (cause) {
      setError(errorText(cause, t("skills.error.load")));
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  async function act(
    id: string,
    name: string,
    action: () => Promise<unknown>,
    success: string,
  ) {
    setBusy(`${id}:${name}`);
    setActionError(null);
    setNotice(null);
    try {
      await action();
      await load();
      setNotice(success);
    } catch (cause) {
      setActionError(errorText(cause, t("skills.error.action")));
    } finally {
      setBusy(null);
    }
  }

  async function reviewLicense(skill: AgenticSkillRecord) {
    const license = licenses[skill.id]?.trim();
    if (!license) {
      setActionError(t("skills.license.required"));
      return;
    }
    await act(
      skill.id,
      "license",
      () => coreApi.reviewAgenticSkillLicense(skill.id, license),
      t("skills.license.done"),
    );
  }

  async function promote(
    skill: AgenticSkillRecord,
    scope: "PROJECT" | "GLOBAL",
  ) {
    await act(
      skill.id,
      `promote-${scope}`,
      () =>
        coreApi.promoteAgenticSkill(skill.id, {
          scope,
          owner_approved: ownerApproved[skill.id] === true,
          confirm: true,
        }),
      scope === "GLOBAL"
        ? t("skills.promote.globalDone")
        : t("skills.promote.projectDone"),
    );
  }

  async function revoke(skill: AgenticSkillRecord) {
    const reason = reasons[skill.id]?.trim();
    if (!reason) {
      setActionError(t("skills.revoke.required"));
      return;
    }
    await act(
      skill.id,
      "revoke",
      () => coreApi.revokeAgenticSkill(skill.id, reason),
      t("skills.revoke.done"),
    );
  }

  if (error) {
    return (
      <div className="page">
        <PageHeader
          title={t("skills.title")}
          description={t("skills.description")}
        />
        <ErrorState
          message={error}
          action={
            <Button variant="secondary" onClick={() => void load()}>
              {t("skills.retry")}
            </Button>
          }
        />
      </div>
    );
  }
  if (!entries) {
    return (
      <div className="page">
        <PageHeader
          title={t("skills.title")}
          description={t("skills.description")}
        />
        <LoadingState label={t("skills.loading")} />
      </div>
    );
  }

  return (
    <div className="page">
      <PageHeader
        title={t("skills.title")}
        description={t("skills.description")}
      />
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
      {entries.length === 0 ? (
        <EmptyState
          title={t("skills.empty.title")}
          description={t("skills.empty.description")}
        />
      ) : (
        <div className="skills-list">
          {entries.map(({ skill, reviewDue }) => {
            const source = manifestSource(skill.manifestJson);
            return (
              <Panel key={skill.id} className="skills-card">
                <div className="section-heading">
                  <div>
                    <h2>{skill.name}</h2>
                    <p className="muted">
                      {skill.provenance}
                      {source ? (
                        <>
                          {" · "}
                          <a href={source} target="_blank" rel="noreferrer">
                            {t("skills.source")}
                          </a>
                        </>
                      ) : null}
                    </p>
                  </div>
                  <div className="skills-badges">
                    <Badge tone={scopeTone(skill.scope)}>{skill.scope}</Badge>
                    <Badge
                      tone={
                        skill.status === "ACTIVE"
                          ? "success"
                          : skill.status === "PROMOTION_READY"
                            ? "attention"
                            : "danger"
                      }
                    >
                      {skill.status}
                    </Badge>
                    {reviewDue && (
                      <Badge tone="danger">{t("skills.reviewDue")}</Badge>
                    )}
                  </div>
                </div>
                <dl className="knowledge-facts">
                  <div>
                    <dt>{t("skills.uses")}</dt>
                    <dd>
                      {skill.successCount} / {skill.failureCount}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("skills.license.label")}</dt>
                    <dd>{skill.license}</dd>
                  </div>
                  <div>
                    <dt>{t("skills.nextReview")}</dt>
                    <dd>
                      {skill.nextReviewAt
                        ? formatDate(skill.nextReviewAt)
                        : t("skills.notScheduled")}
                    </dd>
                  </div>
                  <div>
                    <dt>{t("skills.expires")}</dt>
                    <dd>{formatDate(skill.expiresAt)}</dd>
                  </div>
                </dl>
                <div className="knowledge-actions">
                  <Input
                    label={t("skills.license.input")}
                    value={licenses[skill.id] ?? ""}
                    onChange={(event) =>
                      setLicenses((current) => ({
                        ...current,
                        [skill.id]: event.target.value,
                      }))
                    }
                    placeholder="MIT"
                  />
                  <Button
                    variant="secondary"
                    size="sm"
                    busy={busy === `${skill.id}:license`}
                    disabled={busy !== null}
                    onClick={() => void reviewLicense(skill)}
                  >
                    {t("skills.license.action")}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    busy={busy === `${skill.id}:review`}
                    disabled={
                      busy !== null ||
                      skill.status === "REVOKED" ||
                      skill.status === "EXPIRED"
                    }
                    onClick={() =>
                      void act(
                        skill.id,
                        "review",
                        () => coreApi.reviewAgenticSkill(skill.id),
                        t("skills.review.done"),
                      )
                    }
                  >
                    {t("skills.review.action")}
                  </Button>
                  {skill.scope === "RUN" &&
                    skill.status === "PROMOTION_READY" && (
                      <Button
                        size="sm"
                        busy={busy === `${skill.id}:promote-PROJECT`}
                        disabled={busy !== null}
                        onClick={() => void promote(skill, "PROJECT")}
                      >
                        {t("skills.promote.project")}
                      </Button>
                    )}
                  {skill.scope === "PROJECT" && (
                    <>
                      <Checkbox
                        label={t("skills.promote.owner")}
                        checked={ownerApproved[skill.id] === true}
                        onChange={(event) =>
                          setOwnerApproved((current) => ({
                            ...current,
                            [skill.id]: event.target.checked,
                          }))
                        }
                      />
                      <Button
                        size="sm"
                        busy={busy === `${skill.id}:promote-GLOBAL`}
                        disabled={
                          busy !== null || ownerApproved[skill.id] !== true
                        }
                        onClick={() => void promote(skill, "GLOBAL")}
                      >
                        {t("skills.promote.global")}
                      </Button>
                    </>
                  )}
                </div>
                <div className="knowledge-actions">
                  <Input
                    label={t("skills.revoke.reason")}
                    value={reasons[skill.id] ?? ""}
                    onChange={(event) =>
                      setReasons((current) => ({
                        ...current,
                        [skill.id]: event.target.value,
                      }))
                    }
                  />
                  <Button
                    variant="danger"
                    size="sm"
                    busy={busy === `${skill.id}:revoke`}
                    disabled={
                      busy !== null ||
                      skill.status === "REVOKED" ||
                      skill.status === "EXPIRED"
                    }
                    onClick={() => void revoke(skill)}
                  >
                    {t("skills.revoke.action")}
                  </Button>
                </div>
              </Panel>
            );
          })}
        </div>
      )}
    </div>
  );
}
