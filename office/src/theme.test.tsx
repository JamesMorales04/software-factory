import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { ThemeProvider, useTheme } from "./theme";

function Probe() {
  const { theme, density, setTheme, setDensity } = useTheme();
  return (
    <div>
      <span data-testid="theme">{theme}</span>
      <span data-testid="density">{density}</span>
      <button type="button" onClick={() => setTheme("dark")}>
        dark
      </button>
      <button type="button" onClick={() => setDensity("compact")}>
        compact
      </button>
    </div>
  );
}

describe("ThemeProvider", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.dataset.theme = "system";
    document.documentElement.dataset.density = "comfortable";
  });

  it("applies and persists explicit theme and density choices", async () => {
    render(
      <ThemeProvider>
        <Probe />
      </ThemeProvider>,
    );
    await userEvent.click(screen.getByRole("button", { name: "dark" }));
    await userEvent.click(screen.getByRole("button", { name: "compact" }));
    expect(document.documentElement.dataset.theme).toBe("dark");
    expect(document.documentElement.dataset.density).toBe("compact");
    expect(localStorage.getItem("factory-office-theme")).toBe("dark");
    expect(localStorage.getItem("factory-office-density")).toBe("compact");
    expect(screen.getByTestId("theme").textContent).toBe("dark");
    expect(screen.getByTestId("density").textContent).toBe("compact");
  });
});
