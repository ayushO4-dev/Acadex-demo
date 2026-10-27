"use server";

import { createHmac, randomUUID } from "node:crypto";
import { redirect } from "next/navigation";

import { capture } from "@/lib/mock-gateway";

// The gateway's side of a payment, simulated: it sends the webhook to the API,
// signed as a real gateway would, then returns the browser to the site.
// Local development only (MOCK_GATEWAY); Razorpay test mode replaces it.

const API_URL = process.env.ACADEX_API_URL ?? "http://localhost:8080";

export async function completeCheckout(form: FormData): Promise<void> {
  if (process.env.MOCK_GATEWAY !== "true") throw new Error("mock gateway is disabled");
  const secret = process.env.PAYMENT_WEBHOOK_SECRET;
  if (!secret) throw new Error("PAYMENT_WEBHOOK_SECRET is not set");

  const orderId = String(form.get("order_id"));
  const attempt = String(form.get("attempt"));
  const amount = Number(form.get("amount"));
  const method = String(form.get("method") ?? "upi");
  const failed = form.get("outcome") === "fail";
  const scenario = String(form.get("scenario") ?? "normal");

  // What the gateway takes: once, or (a gateway fault) twice for the order.
  const payments = failed
    ? [{ id: `pay_${randomUUID().replaceAll("-", "").slice(0, 14)}` }]
    : [capture(orderId, amount, method), ...(scenario === "capture_twice" ? [capture(orderId, amount, method)] : [])];

  // "lose_webhook": the money is taken but Acadex is never told; the
  // reconciliation job finds it within five minutes.
  if (scenario !== "lose_webhook") {
    for (const payment of payments) {
      const body = JSON.stringify({
        id: `evt_${randomUUID()}`,
        type: failed ? "payment.failed" : "payment.captured",
        data: { payment_id: payment.id, order_id: orderId, amount, currency: "INR", method },
      });
      // Gateways retry deliveries; "deliver twice" shows that a repeat does nothing.
      const deliveries = scenario === "deliver_twice" ? 2 : 1;
      for (let i = 0; i < deliveries; i++) {
        const timestamp = Math.floor(Date.now() / 1000);
        const signature = createHmac("sha256", secret).update(`${timestamp}.${body}`).digest("hex");
        const response = await fetch(`${API_URL}/webhooks/payments`, {
          method: "POST",
          headers: { "content-type": "application/json", "x-gateway-signature": `t=${timestamp},v1=${signature}` },
          body,
          cache: "no-store",
        });
        console.info(`[mock gateway] ${payment.id} webhook ${i + 1}/${deliveries}: ${response.status} ${await response.text()}`);
      }
    }
  }

  redirect(`/fees/payable?attempt=${encodeURIComponent(attempt)}`);
}
