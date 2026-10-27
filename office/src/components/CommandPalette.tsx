import * as Dialog from "@radix-ui/react-dialog";
import { CornerDownLeft, Search } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { NAV_ITEMS } from "../navigation";

export function CommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return NAV_ITEMS;
    return NAV_ITEMS.filter((item) =>
      `${item.label} ${item.keywords} ${item.to}`
        .toLowerCase()
        .includes(needle),
    );
  }, [query]);

  function choose(to: string) {
    onOpenChange(false);
    setQuery("");
    navigate(to);
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        setActiveIndex(0);
        if (!next) setQuery("");
        onOpenChange(next);
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="command-overlay" />
        <Dialog.Content
          className="command-dialog"
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            inputRef.current?.focus();
          }}
        >
          <Dialog.Title className="ui-visually-hidden">
            Command navigation
          </Dialog.Title>
          <Dialog.Description className="ui-visually-hidden">
            Search implemented Office pages and open one.
          </Dialog.Description>
          <div className="command-input-row">
            <Search size={18} aria-hidden="true" />
            <input
              ref={inputRef}
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActiveIndex(0);
              }}
              role="combobox"
              aria-expanded="true"
              aria-controls="command-results"
              aria-activedescendant={
                results.length > 0 ? `command-option-${activeIndex}` : undefined
              }
              aria-autocomplete="list"
              onKeyDown={(event) => {
                if (event.key === "ArrowDown") {
                  event.preventDefault();
                  setActiveIndex((index) =>
                    results.length === 0 ? 0 : (index + 1) % results.length,
                  );
                } else if (event.key === "ArrowUp") {
                  event.preventDefault();
                  setActiveIndex((index) =>
                    results.length === 0
                      ? 0
                      : (index - 1 + results.length) % results.length,
                  );
                } else if (event.key === "Enter") {
                  const selected = results[activeIndex] ?? results[0];
                  if (selected) {
                    event.preventDefault();
                    choose(selected.to);
                  }
                }
              }}
              placeholder="Search implemented pages…"
              aria-label="Search implemented pages"
              autoComplete="off"
            />
            <kbd className="ui-kbd">Esc</kbd>
          </div>
          <div
            className="command-results"
            id="command-results"
            role="listbox"
            aria-label="Pages"
          >
            {results.length === 0 ? (
              <p className="command-empty">No implemented page matches.</p>
            ) : (
              results.map((item, index) => {
                const Icon = item.icon;
                return (
                  <button
                    type="button"
                    role="option"
                    id={`command-option-${index}`}
                    aria-selected={index === activeIndex}
                    tabIndex={-1}
                    className="command-result"
                    key={item.to}
                    onMouseEnter={() => setActiveIndex(index)}
                    onClick={() => choose(item.to)}
                  >
                    <Icon size={17} aria-hidden="true" />
                    <span>{item.label}</span>
                    <code>{item.to}</code>
                    <CornerDownLeft size={14} aria-hidden="true" />
                  </button>
                );
              })
            )}
          </div>
          <p className="command-hint">
            Navigation only. Office actions remain on their authenticated page.
          </p>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
