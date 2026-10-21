export function renderEntry(entry) {
  const article = document.createElement("article");
  article.className = "entry";
  article.dataset.category = entry.category;
  article.dataset.entryId = entry.id;

  const title = document.createElement("h2");
  title.textContent = entry.title;

  const meta = document.createElement("p");
  meta.className = "meta";
  meta.textContent = `${entry.category} · ${entry.duration}`;

  const summary = document.createElement("p");
  summary.textContent = entry.summary;

  article.append(title, meta, summary);
  return article;
}

export function renderEntries(container, entries) {
  container.replaceChildren(...entries.map(renderEntry));
}
