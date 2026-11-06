import type { Locale, TranslationParams } from "./types";

export type LocaleCatalogs = Record<Locale, Record<string, string>>;

export const INTERPOLATION_PATTERN = /\{(\w+)\}/g;

export function interpolate(
  template: string,
  params: TranslationParams = {},
): string {
  return template.replace(
    INTERPOLATION_PATTERN,
    (placeholder, name: string) => {
      const value = params[name];
      return value === undefined ? placeholder : String(value);
    },
  );
}

export function placeholderNames(template: string): string[] {
  const names = new Set<string>();
  for (const match of template.matchAll(INTERPOLATION_PATTERN)) {
    if (match[1]) names.add(match[1]);
  }
  return [...names].sort();
}

/**
 * Resolve a message in the selected locale, then fall back to English.
 * A key missing from both catalogs returns the key itself and reports a
 * diagnostic so tests and development builds fail visibly instead of silently
 * rendering an empty label.
 */
export function translateString(
  locale: Locale,
  key: string,
  params: TranslationParams,
  catalogs: LocaleCatalogs,
  onMissingKey?: (key: string, locale: Locale) => void,
): string {
  const template = catalogs[locale][key] ?? catalogs.en[key];
  if (template === undefined) {
    onMissingKey?.(key, locale);
    return key;
  }
  return interpolate(template, params);
}

export function intlLocale(locale: Locale): string {
  return locale === "es" ? "es-ES" : "en-US";
}

export function formatNumberFor(
  locale: Locale,
  value: number | bigint,
  options?: Intl.NumberFormatOptions,
): string {
  return new Intl.NumberFormat(intlLocale(locale), options).format(value);
}

export function formatDateFor(
  locale: Locale,
  value: string | number | Date,
  options: Intl.DateTimeFormatOptions = {
    dateStyle: "medium",
    timeStyle: "short",
  },
): string {
  const date = value instanceof Date ? value : new Date(value);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(intlLocale(locale), options).format(date);
}
