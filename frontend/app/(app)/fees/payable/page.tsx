import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Card, CardContent } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { getMe, getPayable, getPaymentStatus } from "@/lib/acadex";
import type { Payable, PaymentStatus } from "@/lib/api";
import { academicYear, formatPaise } from "@/lib/utils";

import { PayButton } from "./pay-button";
import { PaymentBanner } from "./payment-banner";

export const dynamic = "force-dynamic";

const DATE = new Intl.DateTimeFormat("en-IN", { dateStyle: "medium" });

export default async function PayablePage({ searchParams }: { searchParams: Promise<{ attempt?: string }> }) {
  const me = await getMe();
  if (!me.permissions.includes("fees:read")) {
    return (
      <>
        <PageHeader title="Payable" />
        <p className="text-muted-foreground">Fees are shown to students. Collection views belong to the accounts office.</p>
      </>
    );
  }

  const { attempt } = await searchParams;
  let payable: Payable;
  let payment: PaymentStatus | null = null;
  try {
    [payable, payment] = await Promise.all([getPayable(), attempt ? getPaymentStatus(attempt) : null]);
  } catch (err) {
    return (
      <>
        <PageHeader title="Payable" />
        <ApiProblem error={err} />
      </>
    );
  }

  const outstanding = payable.lines.filter((line) => line.pending > 0);
  return (
    <>
      <PageHeader title="Payable" description={`Academic year ${academicYear(payable.academic_year)}`}
        actions={
          <>
            {outstanding.length > 1 && (
              <PayButton
                chargeIds={outstanding.map((line) => line.charge_id)}
                label={`Pay all ${formatPaise(payable.totals.pending)}`}
                variant="default"
              />
            )}
          </>
        }
      />

      {payment && <PaymentBanner payment={payment} />}

      <Card>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Particulars</TableHead>
                <TableHead className="text-right">Receivable</TableHead>
                <TableHead className="text-right">Pending</TableHead>
                <TableHead className="text-right">Paid</TableHead>
                <TableHead className="text-right">Concession</TableHead>
                <TableHead>Remarks</TableHead>
                <TableHead className="text-right">
                  <span className="sr-only">Pay</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody className="tabular-nums">
              {payable.lines.map((line) => (
                <TableRow key={line.charge_id}>
                  <TableCell>
                    <div className="font-medium">{line.particulars}</div>
                    {line.due_on && line.pending > 0 && (
                      <div className="text-muted-foreground text-xs">Due {DATE.format(new Date(line.due_on))}</div>
                    )}
                  </TableCell>
                  <TableCell className="text-right">{formatPaise(line.receivable)}</TableCell>
                  <TableCell className={line.pending > 0 ? "text-right font-semibold" : "text-muted-foreground text-right"}>
                    {line.pending < 0 ? `Advance ${formatPaise(-line.pending)}` : formatPaise(line.pending)}
                  </TableCell>
                  <TableCell className="text-right">{formatPaise(line.paid)}</TableCell>
                  <TableCell className="text-right">{line.concession ? formatPaise(line.concession) : "—"}</TableCell>
                  <TableCell className="text-muted-foreground text-sm">
                    {line.remarks ?? (line.pending === 0 ? "Paid" : "")}
                  </TableCell>
                  <TableCell className="text-right">
                    {line.pending > 0 && <PayButton chargeIds={[line.charge_id]} label="Pay" />}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
            <TableFooter className="tabular-nums">
              <TableRow>
                <TableCell>Total</TableCell>
                <TableCell className="text-right">{formatPaise(payable.totals.receivable)}</TableCell>
                <TableCell className="text-right">{formatPaise(payable.totals.pending)}</TableCell>
                <TableCell className="text-right">{formatPaise(payable.totals.paid)}</TableCell>
                <TableCell className="text-right">{formatPaise(payable.totals.concession)}</TableCell>
                <TableCell colSpan={2} />
              </TableRow>
            </TableFooter>
          </Table>
        </CardContent>
      </Card>
    </>
  );
}
