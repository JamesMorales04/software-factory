export const SUPPORTED_LOCALES = ["en", "es"] as const;

export type Locale = (typeof SUPPORTED_LOCALES)[number];

export type TranslationParams = Record<string, string | number | undefined>;

export type Translate = (
  key: import("./catalog.en").MessageKey,
  params?: TranslationParams,
) => string;

export const LOCALE_STORAGE_KEY = "factory-office-locale";

export const LOCALE_LABELS: Record<Locale, string> = {
  en: "English",
  es: "Español",
};
