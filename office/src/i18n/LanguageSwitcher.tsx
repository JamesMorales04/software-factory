import { Globe } from "lucide-react";
import { useId } from "react";
import { useI18n } from "./I18nProvider";
import { LOCALE_LABELS, type Locale, SUPPORTED_LOCALES } from "./types";

const SHORT_LABEL: Record<Locale, string> = {
  en: "EN",
  es: "ES",
};

/**
 * Language control. The full variant is the labelled Settings block; the
 * compact variant is the global header switcher. Both use a native select so
 * they stay keyboard operable on desktop and touch devices without a custom
 * popup. The header control gets its own accessible name to avoid duplicating
 * the Settings label when both are mounted on the same page.
 */
export function LanguageSwitcher({
  variant = "full",
}: {
  variant?: "full" | "compact";
}) {
  const { locale, setLocale, t } = useI18n();
  const titleId = useId();
  const selectId = useId();
  const noteId = useId();

  function select(next: string) {
    if (SUPPORTED_LOCALES.includes(next as Locale)) {
      setLocale(next as Locale);
    }
  }

  if (variant === "compact") {
    return (
      <div className="language-switcher-compact">
        <Globe size={16} aria-hidden="true" />
        <label className="ui-visually-hidden" htmlFor={selectId}>
          {t("shell.language.label")}
        </label>
        <select
          id={selectId}
          className="ui-select language-switcher-select"
          value={locale}
          title={LOCALE_LABELS[locale]}
          onChange={(event) => select(event.currentTarget.value)}
        >
          {SUPPORTED_LOCALES.map((option) => (
            <option key={option} value={option} lang={option}>
              {SHORT_LABEL[option]}
            </option>
          ))}
        </select>
      </div>
    );
  }

  return (
    <section className="settings-language" aria-labelledby={titleId}>
      <div className="settings-language-copy">
        <h2 id={titleId}>{t("settings.language.title")}</h2>
        <p className="muted">{t("settings.language.description")}</p>
      </div>
      <div className="settings-language-control">
        <label htmlFor={selectId}>{t("settings.language.label")}</label>
        <select
          id={selectId}
          className="ui-select settings-language-select"
          value={locale}
          aria-describedby={noteId}
          onChange={(event) => select(event.currentTarget.value)}
        >
          {SUPPORTED_LOCALES.map((option) => (
            <option key={option} value={option} lang={option}>
              {LOCALE_LABELS[option]}
            </option>
          ))}
        </select>
        <span className="settings-language-note" id={noteId}>
          {t("settings.language.note")}
        </span>
      </div>
    </section>
  );
}
