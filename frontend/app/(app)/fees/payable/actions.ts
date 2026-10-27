"use server";

import { asUser } from "@/lib/acadex";
import { ApiError, type Checkout } from "@/lib/api";

export type StartResult = { ok: true; checkoutUrl: string } | { ok: false; message: string };

/**
 * Opens a payment for some charges. `idempotencyKey` is made once per Pay
 * button in the browser, so a double click or a retry reuses the same
 * attempt instead of opening a second order.
 */
export async function startPayment(chargeIds: number[], idempotencyKey: string): Promise<StartResult> {
  try {
    const checkout = await asUser<Checkout>("/fees/payments", {
      method: "POST",
      body: { charge_ids: chargeIds },
      idempotencyKey,
    });
    return { ok: true, checkoutUrl: checkout.checkout_url };
  } catch (err) {
    const problem = err instanceof ApiError ? err.problem : null;
    return { ok: false, message: problem?.title ?? "The Acadex API is unreachable" };
  }
}
