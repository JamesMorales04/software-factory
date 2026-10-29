import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Input, Select, Textarea } from "./Field";

describe("Field controls", () => {
  it("links help and error text and marks invalid inputs", () => {
    render(
      <Input
        id="spend"
        label="Maximum inference spend (USD)"
        help="Covers assessment and review."
        error="Enter a positive USD amount."
        defaultValue="-1"
      />,
    );
    const input = screen.getByLabelText("Maximum inference spend (USD)");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(input.getAttribute("aria-describedby")).toBe(
      "spend-help spend-error",
    );
    expect(screen.getByText("Covers assessment and review.")).toBeDefined();
    expect(screen.getByRole("alert").textContent).toBe(
      "Enter a positive USD amount.",
    );
  });

  it("keeps an accessible name when the visible prompt is the composer heading", () => {
    render(
      <Textarea
        id="objective"
        label="What should the Factory do?"
        labelHidden
        placeholder="Describe the outcome"
      />,
    );
    const textarea = screen.getByLabelText("What should the Factory do?");
    const label = document.querySelector(`label[for="objective"]`);
    expect(label?.className).toContain("ui-visually-hidden");
    expect(textarea.getAttribute("placeholder")).toBe("Describe the outcome");
  });

  it("supports keyboard focus and disabled selection controls", async () => {
    render(
      <>
        <Input label="Owner" />
        <Select label="Execution profile" disabled defaultValue="">
          <option value="">Core default</option>
        </Select>
      </>,
    );
    await userEvent.tab();
    expect(document.activeElement).toBe(screen.getByLabelText("Owner"));
    const select = screen.getByLabelText("Execution profile");
    expect((select as HTMLSelectElement).disabled).toBe(true);
  });
});
