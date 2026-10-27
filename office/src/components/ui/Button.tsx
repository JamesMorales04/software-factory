import { type ButtonHTMLAttributes, forwardRef, type ReactNode } from "react";
import { cn } from "./cn";

type Variant = "primary" | "secondary" | "quiet" | "danger";
type Size = "sm" | "md";

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
  }
>(function Button(
  { variant = "secondary", size = "md", className, type = "button", ...props },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={buttonClass(variant, size, className)}
      {...props}
    />
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
