import { en } from "./catalog.en";
import { es } from "./catalog.es";
import type { Locale } from "./types";

export type { Catalog, MessageKey } from "./catalog.en";
export { en, es };

export const catalogs = {
  en,
  es,
} satisfies Record<Locale, Record<string, string>>;
