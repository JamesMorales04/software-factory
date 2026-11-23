import type { ReactNode } from "react";

/**
 * Identified, non-scrolling table region. Tables use a fixed desktop layout and
 * a stacked responsive layout below 960px, so no keyboard focus stop is needed
 * to reach horizontally scrolled content.
 */
export function DirectorTableRegion({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <section className="director-table-wrap" aria-label={label}>
      {children}
    </section>
  );
}
