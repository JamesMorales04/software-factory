import { type KeyboardEvent, useEffect, useRef } from "react";
import { cn } from "./cn";

export interface TabItem {
  id: string;
  label: string;
  count?: number;
}

export function Tabs({
  items,
  value,
  onChange,
  label,
  className,
}: {
  items: TabItem[];
  value: string;
  onChange: (id: string) => void;
  label: string;
  className?: string;
}) {
  const refs = useRef<Record<string, HTMLButtonElement | null>>({});

  useEffect(() => {
    const element = refs.current[value];
    if (element && typeof element.scrollIntoView === "function") {
      element.scrollIntoView({ block: "nearest", inline: "nearest" });
    }
  }, [value]);

  function select(index: number) {
    const item = items[index];
    if (!item) return;
    onChange(item.id);
    refs.current[item.id]?.focus();
  }

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.key === "ArrowRight") {
      event.preventDefault();
      select((index + 1) % items.length);
    } else if (event.key === "ArrowLeft") {
      event.preventDefault();
      select((index - 1 + items.length) % items.length);
    } else if (event.key === "Home") {
      event.preventDefault();
      select(0);
    } else if (event.key === "End") {
      event.preventDefault();
      select(items.length - 1);
    }
  }

  return (
    <div className={cn("ui-tabs", className)} role="tablist" aria-label={label}>
      {items.map((item, index) => {
        const selected = item.id === value;
        return (
          <button
            key={item.id}
            ref={(element) => {
              refs.current[item.id] = element;
            }}
            type="button"
            role="tab"
            id={`tab-${item.id}`}
            aria-selected={selected}
            aria-controls={`panel-${item.id}`}
            tabIndex={selected ? 0 : -1}
            className={cn("ui-tab", selected && "ui-tab--active")}
            onClick={() => onChange(item.id)}
            onKeyDown={(event) => onKeyDown(event, index)}
          >
            <span>{item.label}</span>
            {item.count !== undefined && (
              <span className="ui-tab-count">{item.count}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}
