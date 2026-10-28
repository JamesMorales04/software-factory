import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, useLocation } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { en, es, type MessageKey } from "./catalog";
import { I18nProvider } from "./I18nProvider";
import { LOCALE_STORAGE_KEY, useI18n } from "./index";
import { LanguageSwitcher } from "./LanguageSwitcher";
import {
  formatDateFor,
  formatNumberFor,
  placeholderNames,
  translateString,
} from "./translate";

describe("localization catalogs", () => {
  it("keeps English and Spanish keys and placeholders in exact parity", () => {
    const englishKeys = Object.keys(en).sort() as MessageKey[];
    const spanishKeys = Object.keys(es).sort() as MessageKey[];
    expect(spanishKeys).toEqual(englishKeys);

    for (const key of englishKeys) {
      expect(es[key].trim(), `Spanish message ${key}`).not.toBe("");
      expect(placeholderNames(en[key]), `placeholders for ${key}`).toEqual(
        placeholderNames(es[key]),
      );
    }
  });

  it("interpolates named parameters and falls back to English", () => {
    const catalogs = {
      en: { "projects.register.notice": "Registered {id} at {commit}." },
      es: {},
    };
    expect(
      translateString(
        "es",
        "projects.register.notice",
        { id: "project-1", commit: "0123456" },
        catalogs,
      ),
    ).toBe("Registered project-1 at 0123456.");

    const diagnostics: string[] = [];
    expect(
      translateString("es", "missing.key", {}, catalogs, (key, locale) =>
        diagnostics.push(`${locale}:${key}`),
      ),
    ).toBe("missing.key");
    expect(diagnostics).toEqual(["es:missing.key"]);
  });

  it("formats numbers and dates with Intl for each locale", () => {
    expect(formatNumberFor("en", 1234567.89)).toBe("1,234,567.89");
    expect(formatNumberFor("es", 1234567.89)).toBe("1.234.567,89");

    const options: Intl.DateTimeFormatOptions = {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      timeZone: "UTC",
    };
    expect(formatDateFor("en", "2026-10-27T10:00:00Z", options)).toBe(
      "10/27/2026",
    );
    expect(formatDateFor("es", "2026-10-27T10:00:00Z", options)).toBe(
      "27/10/2026",
    );
    expect(formatDateFor("es", "not-a-date")).toBe("—");
  });
});

function Probe() {
  const { locale, t, formatNumber } = useI18n();
  return (
    <>
      <p>{t("settings.language.title")}</p>
      <p data-testid="locale">{locale}</p>
      <p data-testid="number">{formatNumber(12345)}</p>
      <LanguageSwitcher />
    </>
  );
}

function RouteProbe() {
  const location = useLocation();
  return (
    <output data-testid="route">
      {location.pathname}
      {location.search}
    </output>
  );
}

describe("I18nProvider", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.lang = "en";
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("defaults to English, switches to Spanish and persists across remount", async () => {
    const user = userEvent.setup();
    const view = render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );

    expect(screen.getAllByText("Language").length).toBeGreaterThan(0);
    expect(document.documentElement.lang).toBe("en");
    const select = screen.getByLabelText("Interface language");

    await user.selectOptions(select, "es");

    expect(screen.getAllByText("Idioma").length).toBeGreaterThan(0);
    expect(screen.getByTestId("locale").textContent).toBe("es");
    expect(screen.getByTestId("number").textContent).toBe("12.345");
    await waitFor(() => expect(document.documentElement.lang).toBe("es"));
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe("es");

    view.unmount();
    render(
      <I18nProvider>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getAllByText("Idioma").length).toBeGreaterThan(0);
    expect(screen.getByTestId("locale").textContent).toBe("es");
  });

  it("keeps the language select keyboard reachable and labelled", async () => {
    const user = userEvent.setup();
    render(
      <I18nProvider initialLocale="en">
        <LanguageSwitcher />
      </I18nProvider>,
    );

    await user.tab();
    expect(document.activeElement).toBe(
      screen.getByLabelText("Interface language"),
    );
  });

  it("switches language without changing the current route or context", async () => {
    const user = userEvent.setup();
    render(
      <MemoryRouter initialEntries={["/runtime?tab=diagnostics"]}>
        <I18nProvider initialLocale="en">
          <RouteProbe />
          <LanguageSwitcher />
        </I18nProvider>
      </MemoryRouter>,
    );

    await user.selectOptions(screen.getByLabelText("Interface language"), "es");

    expect(screen.getByTestId("route").textContent).toBe(
      "/runtime?tab=diagnostics",
    );
    expect(screen.getAllByText("Idioma").length).toBeGreaterThan(0);
  });

  it("reports a missing key in tests and renders the key instead of blank UI", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    function MissingProbe() {
      const { t } = useI18n();
      return <span>{t("test.missing" as MessageKey)}</span>;
    }
    render(
      <I18nProvider initialLocale="es">
        <MissingProbe />
      </I18nProvider>,
    );

    expect(screen.getByText("test.missing")).toBeDefined();
    expect(error).toHaveBeenCalledWith(
      expect.stringContaining('Missing message key "test.missing"'),
    );
  });
});
