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
});
