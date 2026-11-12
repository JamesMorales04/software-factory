import { expect, test } from "vitest";
import { userEvent } from "vitest/browser";
import { render } from "vitest-browser-react";
import { Button } from "./Button";

test("keeps its accessible name while busy and is focusable", async () => {
  const screen = await render(
    <>
      <Button>First</Button>
      <Button variant="primary">Save</Button>
    </>,
  );
  const first = screen.getByRole("button", { name: "First" });
  const button = screen.getByRole("button", { name: "Save" });

  await expect.element(button).toBeVisible();
  await first.click();
  await userEvent.keyboard("{Tab}");
  await expect.element(button).toHaveFocus();

  await screen.rerender(
    <Button variant="primary" busy>
      Save
    </Button>,
  );
  await expect.element(button).toHaveAttribute("aria-busy", "true");
  await expect.element(button).toHaveAccessibleName("Save");
  await expect.element(button).toBeEnabled();
});

test("disabled state is exposed to assistive technology", async () => {
  const screen = await render(
    <Button variant="danger" disabled>
      Delete
    </Button>,
  );
  await expect
    .element(screen.getByRole("button", { name: "Delete" }))
    .toBeDisabled();
});
