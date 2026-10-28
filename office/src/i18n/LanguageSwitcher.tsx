import { useId } from "react";
import { useI18n } from "./I18nProvider";
import { LOCALE_LABELS, type Locale, SUPPORTED_LOCALES } from "./types";

/**
 * Compact language control. The native select keeps the control keyboard
 * operable on desktop and touch devices without a custom popup.
 */
export function LanguageSwitcher() {
  const { locale, setLocale, t } = useI18n();
  const titleId = useId();
  const selectId = useId();
  const noteId = useId();

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
          onChange={(event) => {
            const next = event.currentTarget.value;
            if (SUPPORTED_LOCALES.includes(next as Locale)) {
              setLocale(next as Locale);
            }
          }}
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
