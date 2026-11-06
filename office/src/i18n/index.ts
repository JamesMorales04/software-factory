export type { Catalog, MessageKey } from "./catalog";
export { catalogs, en, es } from "./catalog";
export type { I18nContextValue } from "./I18nProvider";
export { I18nProvider, useI18n } from "./I18nProvider";
export { LanguageSwitcher } from "./LanguageSwitcher";
export type { LocaleCatalogs } from "./translate";
export {
  formatDateFor,
  formatNumberFor,
  INTERPOLATION_PATTERN,
  interpolate,
  intlLocale,
  placeholderNames,
  translateString,
} from "./translate";
export type { Locale, Translate, TranslationParams } from "./types";
export {
  LOCALE_LABELS,
  LOCALE_STORAGE_KEY,
  SUPPORTED_LOCALES,
} from "./types";
