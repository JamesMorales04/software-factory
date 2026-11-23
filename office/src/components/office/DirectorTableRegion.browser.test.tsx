import { expect, test } from "vitest";
import { page } from "vitest/browser";
import { render } from "vitest-browser-react";
import "../../views/director.css";
import { DirectorTableRegion } from "./DirectorTableRegion";

const longValue =
  "github:NixOS/nixpkgs/ac62194c3917d5f474c1a844b6fd6da2db95077d/candidate/src/very/deep/path/with/a/long/unbroken/identifier";

test("keeps table content reachable without horizontal overflow or a focus trap", async () => {
  await render(
    <DirectorTableRegion label="Agent assignments">
      <table className="director-table">
        <thead>
          <tr>
            <th scope="col">Role</th>
            <th scope="col">Specialization</th>
            <th scope="col">Count</th>
            <th scope="col">Access</th>
            <th scope="col">Tools</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <th scope="row">implementer</th>
            <td data-label="Specialization">{longValue}</td>
            <td data-label="Count">12</td>
            <td data-label="Access">write</td>
            <td data-label="Tools">run_command, write_file</td>
          </tr>
        </tbody>
      </table>
    </DirectorTableRegion>,
  );

  for (const width of [320, 390, 834, 1440]) {
    await page.viewport(width, 900);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const region = document.querySelector(".director-table-wrap");
    expect(region).not.toBeNull();
    expect(region?.tagName).toBe("SECTION");
    expect(region?.getAttribute("aria-label")).toBe("Agent assignments");
    expect(region?.getAttribute("tabindex")).toBeNull();
    const element = region as HTMLElement;
    expect(element.scrollWidth).toBeLessThanOrEqual(element.clientWidth);
    expect(document.documentElement.scrollWidth).toBeLessThanOrEqual(
      window.innerWidth,
    );

    const head = element.querySelector("thead");
    expect(head).not.toBeNull();
    const display = getComputedStyle(head as HTMLElement).display;
    if (width <= 960) {
      expect(display).toBe("none");
      expect(
        getComputedStyle(element.querySelector("tr td") as HTMLElement).display,
      ).toBe("block");
    } else {
      expect(display).not.toBe("none");
    }
  }
});
