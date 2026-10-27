import type { ResultKind } from "@/lib/api";

// Shared by server pages and client controls. Constants a server component
// uses must not live in a "use client" module: there they'd arrive as client
// references, not values.
export const KIND_LABEL: Record<ResultKind, string> = {
  regular: "Regular",
  backlog: "Backlog",
  revaluation: "Re-evaluation",
};
