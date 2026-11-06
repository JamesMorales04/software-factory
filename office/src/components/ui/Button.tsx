import { LoaderCircle } from "lucide-react";
import { type ButtonHTMLAttributes, forwardRef, type ReactNode } from "react";
import { cn } from "./cn";

type Variant = "primary" | "secondary" | "quiet" | "danger";
type Size = "sm" | "md" | "lg";

export function buttonClass(
  variant: Variant = "secondary",
  size: Size = "md",
  className?: string,
): string {
  return cn(
    "ui-button",
    `ui-button--${variant}`,
    `ui-button--${size}`,
    className,
  );
}

export const Button = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & {
    variant?: Variant;
    size?: Size;
    /**
     * Marks an in-flight Core operation. Callers still pass `disabled` to
     * prevent duplicate submission; `busy` adds the spinner and aria-busy
     * state without changing the button's accessible name.
     */
    busy?: boolean;
  }
>(function Button(
  {
    variant = "secondary",
    size = "md",
    busy = false,
    className,
    children,
    type = "button",
    ...props
  },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={buttonClass(variant, size, className)}
      aria-busy={busy || undefined}
      {...props}
    >
      {busy && (
        <LoaderCircle
          className="ui-button-spinner"
          size={16}
          aria-hidden="true"
        />
      )}
      {children}
    </button>
  );
});

export const IconButton = forwardRef<
  HTMLButtonElement,
  ButtonHTMLAttributes<HTMLButtonElement> & {
    label: string;
    children: ReactNode;
  }
>(function IconButton(
  { label, children, className, type = "button", ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      aria-label={label}
      title={label}
      className={cn("ui-icon-button", className)}
      {...props}
    >
      {children}
    </button>
  );
});
