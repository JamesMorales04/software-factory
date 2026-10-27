import {
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
  useId,
} from "react";
import { cn } from "./cn";

export function Field({
  label,
  htmlFor,
  help,
  error,
  children,
  className,
}: {
  label: ReactNode;
  htmlFor: string;
  help?: ReactNode;
  error?: string | null;
  children: ReactNode;
  className?: string;
}) {
  const helpId = `${htmlFor}-help`;
  const errorId = `${htmlFor}-error`;
  return (
    <div className={cn("ui-field", className)}>
      <label className="ui-field-label" htmlFor={htmlFor}>
        {label}
      </label>
      {children}
      {help && (
        <span className="ui-field-help" id={helpId}>
          {help}
        </span>
      )}
      {error && (
        <span className="ui-field-error" id={errorId} role="alert">
          {error}
        </span>
      )}
    </div>
  );
}

function describedBy(
  id: string,
  help: ReactNode | undefined,
  error: string | null | undefined,
): string | undefined {
  const ids = [help ? `${id}-help` : null, error ? `${id}-error` : null].filter(
    Boolean,
  );
  return ids.length > 0 ? ids.join(" ") : undefined;
}

export function Input({
  label,
  help,
  error,
  id: providedId,
  className,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & {
  label: ReactNode;
  help?: ReactNode;
  error?: string | null;
}) {
  const generated = useId();
  const id = providedId ?? generated;
  return (
    <Field label={label} htmlFor={id} help={help} error={error}>
      <input
        id={id}
        className={cn("ui-input", className)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy(id, help, error)}
        {...props}
      />
    </Field>
  );
}

export function Textarea({
  label,
  help,
  error,
  id: providedId,
  className,
  ...props
}: TextareaHTMLAttributes<HTMLTextAreaElement> & {
  label: ReactNode;
  help?: ReactNode;
  error?: string | null;
}) {
  const generated = useId();
  const id = providedId ?? generated;
  return (
    <Field label={label} htmlFor={id} help={help} error={error}>
      <textarea
        id={id}
        className={cn("ui-textarea", className)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy(id, help, error)}
        {...props}
      />
    </Field>
  );
}

export function Select({
  label,
  help,
  error,
  id: providedId,
  className,
  children,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement> & {
  label: ReactNode;
  help?: ReactNode;
  error?: string | null;
}) {
  const generated = useId();
  const id = providedId ?? generated;
  return (
    <Field label={label} htmlFor={id} help={help} error={error}>
      <select
        id={id}
        className={cn("ui-select", className)}
        aria-invalid={error ? true : undefined}
        aria-describedby={describedBy(id, help, error)}
        {...props}
      >
        {children}
      </select>
    </Field>
  );
}

export function Checkbox({
  label,
  className,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { label: ReactNode }) {
  return (
    <label className={cn("ui-checkbox", className)}>
      <input type="checkbox" {...props} />
      <span>{label}</span>
    </label>
  );
}
