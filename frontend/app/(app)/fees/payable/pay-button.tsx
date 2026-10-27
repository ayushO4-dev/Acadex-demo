"use client";

import { useState, useTransition } from "react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";

import { startPayment } from "./actions";

/**
 * Pays for some charges via the gateway's checkout. The idempotency key is
 * fixed for this button's lifetime: clicking twice, or retrying after an
 * error, reuses the same payment attempt.
 */
export function PayButton({
  chargeIds,
  label,
  variant = "outline",
}: {
  chargeIds: number[];
  label: string;
  variant?: "default" | "outline";
}) {
  const [key] = useState(() => crypto.randomUUID());
  const [pending, startTransition] = useTransition();

  return (
    <Button
      size="sm"
      variant={variant}
      disabled={pending}
      onClick={() =>
        startTransition(async () => {
          const result = await startPayment(chargeIds, key);
          if (result.ok) window.location.assign(result.checkoutUrl);
          else toast.error(result.message);
        })
      }
    >
      {pending ? "Opening checkout…" : label}
    </Button>
  );
}
