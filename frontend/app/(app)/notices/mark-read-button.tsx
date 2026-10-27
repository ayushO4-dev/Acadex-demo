"use client";

import { useTransition } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";

import { markRead } from "./actions";

export function MarkReadButton({ ids }: { ids: string[] }) {
  const [pending, startTransition] = useTransition();
  return (
    <Button
      variant="outline"
      size="sm"
      disabled={pending}
      onClick={() =>
        startTransition(async () => {
          const result = await markRead(ids);
          if (result && !result.ok) toast.error(result.message);
        })
      }
    >
      Mark all as read
    </Button>
  );
}
