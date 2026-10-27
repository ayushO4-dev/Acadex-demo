"use client";

import { CircleCheck, Clock, OctagonX, SearchCheck } from "lucide-react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect } from "react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import type { PaymentStatus } from "@/lib/api";
import { formatPaise } from "@/lib/utils";

/**
 * Shown when the browser comes back from checkout. It reports only what the
 * gateway's webhook has recorded, and checks again every two seconds while
 * that hasn't arrived yet.
 */
export function PaymentBanner({ payment }: { payment: PaymentStatus }) {
  const router = useRouter();
  useEffect(() => {
    if (payment.status !== "created") return;
    const timer = setInterval(() => router.refresh(), 2000);
    return () => clearInterval(timer);
  }, [payment.status, router]);

  const amount = formatPaise(payment.amount_paise);
  switch (payment.status) {
    case "captured":
      return (
        <Alert className="mb-4">
          <CircleCheck />
          <AlertTitle>Payment of {amount} received</AlertTitle>
          <AlertDescription>
            <span>
              Receipt {payment.receipt_no} is under{" "}
              <Link href="/fees/receipts" className="underline underline-offset-4">
                Receipts
              </Link>
              .
            </span>
          </AlertDescription>
        </Alert>
      );
    case "created":
      return (
        <Alert className="mb-4">
          <Clock />
          <AlertTitle>Waiting for the gateway to confirm {amount}</AlertTitle>
          <AlertDescription>This page updates by itself. Don&apos;t pay again.</AlertDescription>
        </Alert>
      );
    case "failed":
      return (
        <Alert variant="destructive" className="mb-4">
          <OctagonX />
          <AlertTitle>The payment of {amount} didn&apos;t go through</AlertTitle>
          <AlertDescription>Nothing was charged. You can try again.</AlertDescription>
        </Alert>
      );
    case "review":
      return (
        <Alert className="mb-4">
          <SearchCheck />
          <AlertTitle>We&apos;re checking your payment of {amount}</AlertTitle>
          <AlertDescription>The accounts office will confirm it shortly.</AlertDescription>
        </Alert>
      );
  }
}
