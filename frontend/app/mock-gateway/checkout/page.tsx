import { FlaskConical } from "lucide-react";
import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { formatPaise } from "@/lib/utils";

import { completeCheckout } from "./actions";
import { getInstitution } from "@/lib/api";

export const metadata: Metadata = { title: "Test gateway checkout" };
export const dynamic = "force-dynamic";

const METHODS = [
  { value: "upi", label: "UPI" },
  { value: "card", label: "Card" },
  { value: "netbanking", label: "Net banking" },
];

/**
 * Stands in for the payment gateway's hosted checkout page in local
 * development. Not part of Acadex: in production the browser goes to the
 * gateway itself, and this route returns 404.
 */
export default async function MockCheckout({
  searchParams,
}: {
  searchParams: Promise<{ order_id?: string; amount?: string; attempt?: string }>;
}) {
  if (process.env.MOCK_GATEWAY !== "true") notFound();
  const { order_id: orderId, amount, attempt } = await searchParams;
  if (!orderId || !amount || !attempt || !/^\d+$/.test(amount)) notFound();

  return (
    <main className="bg-muted/40 flex min-h-screen items-center justify-center p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <div className="mb-2 flex items-center gap-2 rounded-md border border-dashed border-amber-500/60 bg-amber-500/10 px-2 py-1 text-xs text-amber-800 dark:text-amber-300">
            <FlaskConical className="size-3.5" aria-hidden />
            Test gateway — no real money moves
          </div>
          <CardDescription>{(await getInstitution()).name}</CardDescription>
          <CardTitle className="text-3xl tabular-nums">{formatPaise(Number(amount))}</CardTitle>
          <p className="text-muted-foreground font-mono text-xs">{orderId}</p>
        </CardHeader>
        <CardContent>
          <form action={completeCheckout} className="grid min-w-0 gap-4">
            <input type="hidden" name="order_id" value={orderId} />
            <input type="hidden" name="amount" value={amount} />
            <input type="hidden" name="attempt" value={attempt} />
            <fieldset className="grid gap-2">
              <legend className="mb-2 text-sm font-medium">Pay with</legend>
              {METHODS.map((method, index) => (
                <Label key={method.value} className="flex items-center gap-2 rounded-md border p-3 font-normal">
                  <input type="radio" name="method" value={method.value} defaultChecked={index === 0} />
                  {method.label}
                </Label>
              ))}
            </fieldset>
            <div className="grid gap-2">
              <Label htmlFor="scenario" className="text-sm">
                Test scenario
              </Label>
              <select
                id="scenario"
                name="scenario"
                defaultValue="normal"
                className="border-input bg-background h-8 w-full min-w-0 rounded-md border px-2 text-sm"
              >
                <option value="normal">Normal payment</option>
                <option value="deliver_twice">Webhook delivered twice</option>
                <option value="lose_webhook">Webhook lost (reconciliation recovers it)</option>
                <option value="capture_twice">Captured twice (flagged for refund)</option>
              </select>
            </div>
            <Button type="submit" name="outcome" value="pay" size="lg">
              Pay {formatPaise(Number(amount))}
            </Button>
            <Button type="submit" name="outcome" value="fail" variant="outline">
              Simulate a failed payment
            </Button>
          </form>
        </CardContent>
      </Card>
    </main>
  );
}
