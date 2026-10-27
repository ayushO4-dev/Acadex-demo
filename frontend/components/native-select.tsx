import { cn } from "@/lib/utils";

/**
 * A styled native <select>: works in plain server-rendered forms (no client
 * JavaScript needed) and keeps the platform's own accessible picker on mobile.
 */
export function NativeSelect({ className, ...props }: React.ComponentProps<"select">) {
  return (
    <select
      className={cn(
        "border-input bg-background focus-visible:ring-ring/50 h-8 w-full rounded-md border px-3 text-sm shadow-xs outline-none focus-visible:ring-[3px] disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}
