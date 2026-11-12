import { expect, test } from "vitest";
import { render } from "vitest-browser-react";
import { Button } from "./Button";

test("keeps its accessible name while busy and is focusable", async () => {
  const screen = await render(<Button variant="primary">Save</Button>);
  const button = screen.getByRole("button", { name: "Save" });

  await expect.element(button).toBeVisible();
  await button.click();
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
