import { forwardRef } from "react";
import { Input, type InputProps } from "./input";

export interface TextFieldProps extends Omit<InputProps, "value" | "onChange"> {
  label: string;
  value: string;
  onChange: (value: string) => void;
}

/** Single-line field. The accessible name is `label`, matching the old boxed fields. */
export const TextField = forwardRef<HTMLInputElement, TextFieldProps>(function TextField(
  { label, value, onChange, ...props },
  ref,
) {
  return (
    <Input
      ref={ref}
      aria-label={label}
      value={value}
      onChange={(event) => onChange(event.target.value)}
      {...props}
    />
  );
});
