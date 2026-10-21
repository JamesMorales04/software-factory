import { categories, entries } from "./data.js";
import { renderEntries } from "./render.js";

const list = document.querySelector("#entries");
const filters = document.querySelector("#filters");
const empty = document.querySelector("#empty-state");

function render(category) {
  // The baseline renders every entry and ignores the selected category.
  const visible = entries;
  renderEntries(list, visible);
  empty.hidden = visible.length > 0;
  for (const button of filters.querySelectorAll("button")) {
    const active = button.dataset.category === category;
    button.setAttribute("aria-pressed", String(active));
  }
}

for (const category of categories) {
  const button = document.createElement("button");
  button.type = "button";
  button.dataset.category = category;
  button.textContent = category;
  button.addEventListener("click", () => render(category));
  filters.append(button);
}

render("All");
