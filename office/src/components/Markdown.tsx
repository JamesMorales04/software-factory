import type { ReactNode } from "react";

const MARKDOWN_EXTENSIONS = new Set([".md", ".markdown", ".mdown"]);

export function isMarkdownPath(path: string): boolean {
  const lower = path.toLowerCase();
  return [...MARKDOWN_EXTENSIONS].some((extension) =>
    lower.endsWith(extension),
  );
}

/**
 * Render a deliberately bounded Markdown subset as React elements.
 *
 * Raw HTML, embedded HTML, images and arbitrary URL schemes are never
 * interpreted. Unsupported syntax is shown as text, not passed to
 * `dangerouslySetInnerHTML`.
 */
export function Markdown({ source }: { source: string }) {
  return <div className="project-markdown">{renderBlocks(source)}</div>;
}

function renderBlocks(source: string): ReactNode[] {
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let index = 0;
  let key = 0;
  const nextKey = (prefix: string) => `${prefix}-${key++}`;

  while (index < lines.length) {
    const line = lines[index];
    if (line.trim() === "") {
      index += 1;
      continue;
    }

    const fence = line.match(/^\s*(```|~~~)\s*([^\s`]*)\s*$/);
    if (fence) {
      const marker = fence[1];
      const language = fence[2];
      const code: string[] = [];
      index += 1;
      while (index < lines.length) {
        if (lines[index].trim().startsWith(marker)) {
          index += 1;
          break;
        }
        code.push(lines[index]);
        index += 1;
      }
      blocks.push(
        <div className="project-markdown-code" key={`block-${key++}`}>
          {language && (
            <span className="project-markdown-code-lang">{language}</span>
          )}
          <pre>
            <code>{code.join("\n")}</code>
          </pre>
        </div>,
      );
      continue;
    }

    const heading = line.match(/^(#{1,6})\s+(.*)$/);
    if (heading) {
      const level = Math.min(heading[1].length + 1, 6);
      const Tag = `h${level}` as "h2" | "h3" | "h4" | "h5" | "h6";
      blocks.push(<Tag key={`block-${key++}`}>{renderInline(heading[2])}</Tag>);
      index += 1;
      continue;
    }

    if (isHorizontalRule(line)) {
      blocks.push(<hr key={`block-${key++}`} />);
      index += 1;
      continue;
    }

    if (/^\s*>\s?/.test(line)) {
      const quote: string[] = [];
      while (index < lines.length && /^\s*>\s?/.test(lines[index])) {
        quote.push(lines[index].replace(/^\s*>\s?/, ""));
        index += 1;
      }
      blocks.push(
        <blockquote key={`block-${key++}`}>
          <p>{renderInline(quote.join(" "))}</p>
        </blockquote>,
      );
      continue;
    }

    const unordered = line.match(/^\s*[-*+]\s+(.*)$/);
    if (unordered) {
      const items: string[] = [];
      while (index < lines.length) {
        const item = lines[index].match(/^\s*[-*+]\s+(.*)$/);
        if (!item) break;
        items.push(item[1]);
        index += 1;
      }
      blocks.push(
        <ul key={nextKey("list")}>
          {items.map((item) => (
            <li key={nextKey("item")}>{renderInline(item)}</li>
          ))}
        </ul>,
      );
      continue;
    }

    const ordered = line.match(/^\s*(\d+)[.)]\s+(.*)$/);
    if (ordered) {
      const start = Number.parseInt(ordered[1], 10);
      const items: string[] = [];
      while (index < lines.length) {
        const item = lines[index].match(/^\s*\d+[.)]\s+(.*)$/);
        if (!item) break;
        items.push(item[1]);
        index += 1;
      }
      blocks.push(
        <ol key={nextKey("list")} start={start}>
          {items.map((item) => (
            <li key={nextKey("item")}>{renderInline(item)}</li>
          ))}
        </ol>,
      );
      continue;
    }

    if (isTableStart(lines, index)) {
      const header = splitTableRow(lines[index]);
      const rows: string[][] = [];
      index += 2;
      while (index < lines.length && lines[index].includes("|")) {
        rows.push(splitTableRow(lines[index]));
        index += 1;
      }
      blocks.push(
        <div className="project-markdown-table" key={nextKey("table")}>
          <table>
            <thead>
              <tr>
                {header.map((cell) => (
                  <th scope="col" key={nextKey("head")}>
                    {renderInline(cell)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => (
                <tr key={nextKey("row")}>
                  {header.map((_, cellIndex) => (
                    <td key={nextKey("cell")}>
                      {renderInline(row[cellIndex] ?? "")}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
      continue;
    }

    const paragraph: string[] = [];
    while (index < lines.length) {
      if (lines[index].trim() === "" || isBlockStart(lines, index)) break;
      paragraph.push(lines[index].trim());
      index += 1;
    }
    blocks.push(
      <p key={`block-${key++}`}>{renderInline(paragraph.join(" "))}</p>,
    );
  }

  return blocks;
}

function isBlockStart(lines: string[], index: number): boolean {
  const line = lines[index];
  return (
    /^\s*(```|~~~)/.test(line) ||
    /^(#{1,6})\s+/.test(line) ||
    isHorizontalRule(line) ||
    /^\s*>\s?/.test(line) ||
    /^\s*[-*+]\s+/.test(line) ||
    /^\s*\d+[.)]\s+/.test(line) ||
    isTableStart(lines, index)
  );
}

function isHorizontalRule(line: string): boolean {
  return /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line);
}

function isTableStart(lines: string[], index: number): boolean {
  if (index + 1 >= lines.length) return false;
  const header = lines[index];
  const divider = lines[index + 1];
  if (!header.includes("|") || !divider.includes("|")) return false;
  const headerCells = splitTableRow(header);
  const dividerCells = splitTableRow(divider);
  return (
    headerCells.length > 0 &&
    dividerCells.length === headerCells.length &&
    dividerCells.every((cell) => /^:?-{3,}:?$/.test(cell.trim()))
  );
}

function splitTableRow(line: string): string[] {
  return line
    .trim()
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

const INLINE_PATTERN =
  /(`[^`\n]+`|\*\*[^*\n]+\*\*|__[^_\n]+__|\[[^\]\n]+\]\([^)\s]+\)|\*[^*\n]+\*|_[^_\n]+_)/g;

function renderInline(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  let cursor = 0;
  let key = 0;
  const pattern = new RegExp(INLINE_PATTERN.source, "g");
  for (const match of text.matchAll(pattern)) {
    const token = match[0];
    const offset = match.index ?? 0;
    if (offset > cursor) {
      nodes.push(text.slice(cursor, offset));
    }
    nodes.push(renderInlineToken(token, key));
    key += 1;
    cursor = offset + token.length;
  }
  if (cursor < text.length) {
    nodes.push(text.slice(cursor));
  }
  return nodes;
}

function renderInlineToken(token: string, key: number): ReactNode {
  if (token.startsWith("`") && token.endsWith("`")) {
    return <code key={`inline-${key}`}>{token.slice(1, -1)}</code>;
  }
  if (token.startsWith("**") || token.startsWith("__")) {
    return (
      <strong key={`inline-${key}`}>{renderInline(token.slice(2, -2))}</strong>
    );
  }
  if (token.startsWith("*") || token.startsWith("_")) {
    return <em key={`inline-${key}`}>{renderInline(token.slice(1, -1))}</em>;
  }
  const link = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
  if (link) {
    const href = safeHref(link[2]);
    if (href) {
      return (
        <a
          key={`inline-${key}`}
          href={href}
          target="_blank"
          rel="noreferrer noopener"
        >
          {renderInline(link[1])}
        </a>
      );
    }
    return (
      <span
        key={`inline-${key}`}
        className="project-markdown-link-unavailable"
        title="Only absolute http(s) links are rendered; relative and script URLs are not followed."
      >
        {renderInline(link[1])}
      </span>
    );
  }
  return token;
}

function safeHref(value: string): string | null {
  if (!/^https?:\/\//i.test(value)) return null;
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:"
      ? url.href
      : null;
  } catch {
    return null;
  }
}
