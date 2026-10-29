import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Button, IconButton } from "./Button";

describe("Button", () => {
  it("renders a named primary action and invokes it", async () => {
    const onClick = vi.fn();
    render(
      <Button variant="primary" onClick={onClick}>
        Save changes
      </Button>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Save changes" }));
    expect(onClick).toHaveBeenCalledOnce();
    expect(
      screen.getByRole("button", { name: "Save changes" }).className,
    ).toContain("ui-button--primary");
  });

  it("does not invoke a disabled action", async () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Unavailable
      </Button>,
    );
    await userEvent.click(screen.getByRole("button", { name: "Unavailable" }));
    expect(onClick).not.toHaveBeenCalled();
  });

  it("requires an accessible name for icon-only actions", () => {
    render(
      <IconButton label="Lock Office">
        <span aria-hidden="true">L</span>
      </IconButton>,
    );
    expect(
      screen.getByRole("button", { name: "Lock Office" }).getAttribute("title"),
    ).toBe("Lock Office");
  });

  it("exposes an in-flight busy state without changing the accessible name", async () => {
    const onClick = vi.fn();
    const { rerender } = render(
      <Button variant="primary" size="lg">
        Submit request
      </Button>,
    );
    const idle = screen.getByRole("button", { name: "Submit request" });
    expect(idle.getAttribute("aria-busy")).toBeNull();

    rerender(
      <Button variant="primary" size="lg" busy disabled onClick={onClick}>
        Submitting request…
      </Button>,
    );
    const busy = screen.getByRole("button", { name: "Submitting request…" });
    expect(busy.getAttribute("aria-busy")).toBe("true");
    expect(busy.querySelector(".ui-button-spinner")).not.toBeNull();
    expect((busy as HTMLButtonElement).disabled).toBe(true);
    await userEvent.click(busy);
    expect(onClick).not.toHaveBeenCalled();
  });
});
