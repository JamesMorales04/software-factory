import {
  createContext,
  type ReactNode,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from "react";
import { catalogs } from "./catalog";
import {
  formatDateFor,
  formatNumberFor,
  type LocaleCatalogs,
  translateString,
} from "./translate";
import {
  LOCALE_STORAGE_KEY,
  type Locale,
  type Translate,
  type TranslationParams,
} from "./types";

export interface I18nContextValue {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: Translate;
  formatNumber: (
    value: number | bigint,
    options?: Intl.NumberFormatOptions,
  ) => string;
  formatDate: (
    value: string | number | Date,
    options?: Intl.DateTimeFormatOptions,
  ) => string;
  formatMicros: (value: number) => string;
}

function reportMissingKey(key: string, locale: Locale) {
  if (import.meta.env.MODE === "test" || import.meta.env.DEV) {
    console.error(`[i18n] Missing message key "${key}" for locale "${locale}"`);
  }
}

function readStoredLocale(): Locale {
  try {
    return window.localStorage.getItem(LOCALE_STORAGE_KEY) === "es"
      ? "es"
      : "en";
  } catch {
    return "en";
  }
}

function persistLocale(locale: Locale) {
  try {
    window.localStorage.setItem(LOCALE_STORAGE_KEY, locale);
  } catch {
    // The active language still applies for this page session.
  }
}

const defaultTranslate: Translate = (key, params) =>
  translateString(
    "en",
    key,
    params ?? {},
    catalogs as LocaleCatalogs,
    reportMissingKey,
  );

const fallbackValue: I18nContextValue = {
  locale: "en",
  setLocale: () => undefined,
  t: defaultTranslate,
  formatNumber: (value, options) => formatNumberFor("en", value, options),
  formatDate: (value, options) => formatDateFor("en", value, options),
  formatMicros: (value) => `${formatNumberFor("en", value)}µ`,
};

const I18nContext = createContext<I18nContextValue>(fallbackValue);

export function I18nProvider({
  children,
  initialLocale,
}: {
  children: ReactNode;
  initialLocale?: Locale;
}) {
  const [locale, setLocaleState] = useState<Locale>(
    () => initialLocale ?? readStoredLocale(),
  );

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
    persistLocale(next);
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale;
    persistLocale(locale);
  }, [locale]);

  const t = useCallback<Translate>(
    (key: Parameters<Translate>[0], params?: TranslationParams) =>
      translateString(
        locale,
        key,
        params ?? {},
        catalogs as LocaleCatalogs,
        reportMissingKey,
      ),
    [locale],
  );

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      setLocale,
      t,
      formatNumber: (number, options) =>
        formatNumberFor(locale, number, options),
      formatDate: (date, options) => formatDateFor(locale, date, options),
      formatMicros: (micros) => `${formatNumberFor(locale, micros)}µ`,
    }),
    [locale, setLocale, t],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

/**
 * Returns the active translator. Outside an I18nProvider the immutable English
 * fallback keeps isolated component tests and embeds working; application code
 * always runs inside the provider installed by main.tsx.
 */
export function useI18n(): I18nContextValue {
  return useContext(I18nContext);
}
