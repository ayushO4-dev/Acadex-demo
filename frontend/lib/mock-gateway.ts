import { randomBytes, timingSafeEqual } from "node:crypto";
import { mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

// The test payment gateway's memory: orders, the payments it captured and
// its refunds, in Razorpay's shapes. Local development only (MOCK_GATEWAY);
// Razorpay test mode replaces it in Phase 4. Kept in a JSON file so it
// survives a restart: reconciliation compares Acadex against this.

export type Order = { id: string; amount: number; currency: "INR"; receipt: string; status: "created" | "paid"; created_at: number };
export type Payment = {
  id: string;
  entity: "payment";
  order_id: string | null;
  amount: number;
  currency: "INR";
  status: "captured" | "failed" | "refunded";
  method: string;
  amount_refunded: number;
  created_at: number;
};
export type Refund = { id: string; entity: "refund"; payment_id: string; amount: number; status: "processed"; created_at: number; key: string };

type State = { orders: Record<string, Order>; payments: Payment[]; refunds: Refund[] };

const FILE = process.env.MOCK_GATEWAY_STATE ?? ".mock-gateway.json";

export function enabled(): boolean {
  return process.env.MOCK_GATEWAY === "true";
}

function load(): State {
  try {
    return JSON.parse(readFileSync(FILE, "utf8")) as State;
  } catch {
    return { orders: {}, payments: [], refunds: [] };
  }
}

/** Read, change, write atomically. One Node process, synchronous: no interleaving. */
function update<T>(change: (state: State) => T): T {
  const state = load();
  const result = change(state);
  mkdirSync(dirname(FILE), { recursive: true });
  writeFileSync(`${FILE}.tmp`, JSON.stringify(state));
  renameSync(`${FILE}.tmp`, FILE);
  return result;
}

const id = (prefix: string) => `${prefix}_${randomBytes(7).toString("hex")}`;
const now = () => Math.floor(Date.now() / 1000);

/** HTTP Basic with the key id, as the gateway's API takes it. */
export function authorised(request: Request): boolean {
  const key = process.env.PAYMENT_GATEWAY_KEY ?? "";
  const header = request.headers.get("authorization") ?? "";
  if (!key || !header.startsWith("Basic ")) return false;
  const [user] = Buffer.from(header.slice(6), "base64").toString().split(":");
  const a = Buffer.from(user ?? "");
  const b = Buffer.from(key);
  return a.length === b.length && timingSafeEqual(a, b);
}

export function createOrder(amount: number, receipt: string): Order {
  return update((s) => {
    const order: Order = { id: id("order"), amount, currency: "INR", receipt, status: "created", created_at: now() };
    s.orders[order.id] = order;
    return order;
  });
}

export function capture(orderId: string, amount: number, method: string): Payment {
  return update((s) => {
    const payment: Payment = {
      id: id("pay"),
      entity: "payment",
      order_id: orderId,
      amount,
      currency: "INR",
      status: "captured",
      method,
      amount_refunded: 0,
      created_at: now(),
    };
    s.payments.push(payment);
    const order = s.orders[orderId];
    if (order) order.status = "paid";
    return payment;
  });
}

export function listPayments(from: number, to: number, count: number, skip: number): Payment[] {
  return load()
    .payments.filter((p) => p.created_at >= from && p.created_at < to)
    .sort((a, b) => a.created_at - b.created_at)
    .slice(skip, skip + count);
}

export type RefundResult = { ok: true; refund: Refund } | { ok: false; status: number; error: string };

/**
 * Refunds part or all of a payment. The same idempotency key returns the
 * same refund. Payments from before this gateway kept state (the demo's
 * seeded history) are accepted as if it had them.
 */
export function refund(paymentId: string, amount: number, key: string): RefundResult {
  return update((s): RefundResult => {
    const earlier = s.refunds.find((r) => r.key === key && key);
    if (earlier) return { ok: true, refund: earlier };
    const payment = s.payments.find((p) => p.id === paymentId);
    if (payment && payment.amount_refunded + amount > payment.amount) {
      return { ok: false, status: 400, error: "The total refund amount is greater than the payment amount" };
    }
    if (!Number.isInteger(amount) || amount <= 0) return { ok: false, status: 400, error: "invalid amount" };
    if (payment) {
      payment.amount_refunded += amount;
      if (payment.amount_refunded === payment.amount) payment.status = "refunded";
    }
    const made: Refund = { id: id("rfnd"), entity: "refund", payment_id: paymentId, amount, status: "processed", created_at: now(), key };
    s.refunds.push(made);
    return { ok: true, refund: made };
  });
}
