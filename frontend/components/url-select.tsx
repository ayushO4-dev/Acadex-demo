"use client";

import { usePathname, useRouter, useSearchParams } from "next/navigation";

import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";

/**
 * A select whose value lives in the URL (`?param=value`), so the view can be
 * linked to and the back button works. Other query parameters are kept,
 * except `reset`, which depend on this one (e.g. the month when the year changes).
 */
export function UrlSelect({
  param,
  value,
  options,
  label,
  reset = [],
  className,
}: {
  param: string;
  value: string;
  options: { value: string; label: string }[];
  label: string;
  reset?: string[];
  className?: string;
}) {
  const router = useRouter();
  const pathname = usePathname();
  const search = useSearchParams();
  return (
    <Select
      value={value}
      onValueChange={(next) => {
        const query = new URLSearchParams(search);
        query.set(param, next);
        for (const key of reset) query.delete(key);
        router.push(`${pathname}?${query}`, { scroll: false });
      }}
    >
      <SelectTrigger className={cn("w-44", className)} aria-label={label}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem key={o.value} value={o.value}>
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
