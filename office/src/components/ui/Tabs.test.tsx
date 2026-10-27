import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Tabs } from "./Tabs";

describe("Tabs", () => {
  it("selects tabs with pointer and arrow keys", async () => {
    const onChange = vi.fn();
    render(
      <Tabs
        items={[
          { id: "overview", label: "Overview" },
          { id: "units", label: "WorkUnits", count: 2 },
          { id: "gate", label: "Gate" },
        ]}
        value="overview"
        onChange={onChange}
        label="Campaign sections"
      />,
    );
    expect(
      screen
        .getByRole("tab", { name: "Overview" })
        .getAttribute("aria-selected"),
    ).toBe("true");
    await userEvent.click(screen.getByRole("tab", { name: /WorkUnits/ }));
    expect(onChange).toHaveBeenCalledWith("units");
    await userEvent.keyboard("{ArrowRight}");
    expect(onChange).toHaveBeenCalledWith("gate");
    await userEvent.keyboard("{Home}");
    expect(onChange).toHaveBeenCalledWith("overview");
  });
});
