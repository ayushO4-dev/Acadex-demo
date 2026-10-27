"use client";

import { usePathname, useRouter, useSearchParams } from "next/navigation";
import { useEffect, useRef, useState } from "react";

import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

/**
 * An input whose value lives in the URL (`?param=value`), like UrlSelect.
 * Dates apply as soon as they are picked; text waits for a pause in typing.
 * Clearing it removes the parameter. `reset` parameters (a page cursor) are
 * dropped whenever it changes.
 */
export function UrlInput({
  param,
  value,
  label,
  type = "text",
  placeholder,
  reset = [],
  className,
  ...rest
}: {
  param: string;
  value: string;
  label: string;
  type?: "text" | "search" | "date";
  placeholder?: string;
  reset?: string[];
  className?: string;
  min?: string;
  max?: string;
}) {
  const router = useRouter();
  const pathname = usePathname();
  const search = useSearchParams();
  const [draft, setDraft] = useState(value);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  // Back/forward, or another control, changed the URL.
  useEffect(() => setDraft(value), [value]);
  useEffect(() => () => clearTimeout(timer.current), []);

  function apply(next: string) {
    const query = new URLSearchParams(search);
    if (next.trim()) query.set(param, next.trim());
    else query.delete(param);
    for (const key of reset) query.delete(key);
    router.push(`${pathname}?${query}`, { scroll: false });
  }

  return (
    <Input
      type={type}
      value={draft}
      aria-label={label}
      placeholder={placeholder}
      className={cn("w-44", className)}
      onChange={(e) => {
        const next = e.target.value;
        setDraft(next);
        clearTimeout(timer.current);
        if (type === "date") apply(next);
        else timer.current = setTimeout(() => apply(next), 400);
      }}
      {...rest}
    />
  );
}
