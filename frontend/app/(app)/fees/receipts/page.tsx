import { ApiProblem } from "@/components/api-problem";
import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { getMe, getReceipts } from "@/lib/acadex";
import type { Receipt } from "@/lib/api";
import { formatPaise } from "@/lib/utils";

export const dynamic = "force-dynamic";

const DATE = new Intl.DateTimeFormat("en-IN", { dateStyle: "medium", timeZone: "Asia/Kolkata" });

export default async function ReceiptsPage() {
  const me = await getMe();
  if (!me.permissions.includes("fees:read")) {
    return (
      <>
        <PageHeader title="Receipts" />
        <p className="text-muted-foreground">Receipts are shown to students.</p>
      </>
    );
  }

  let receipts: Receipt[];
  try {
    receipts = await getReceipts();
  } catch (err) {
    return (
      <>
        <PageHeader title="Receipts" />
        <ApiProblem error={err} />
      </>
    );
  }

  return (
    <>
      <PageHeader title="Receipts" description="One receipt per payment, numbered without gaps each financial year." />
      <Card>
        <CardContent>
          {receipts.length === 0 ? (
            <p className="text-muted-foreground">No payments yet.</p>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>ID</TableHead>
                  <TableHead>Date</TableHead>
                  <TableHead>Remarks</TableHead>
                  <TableHead className="text-right">Amount</TableHead>
                  <TableHead className="text-right">Download</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {receipts.map((receipt) => (
                  <TableRow key={receipt.id}>
                    <TableCell className="font-mono text-sm">{receipt.receipt_no}</TableCell>
                    <TableCell className="whitespace-nowrap">{DATE.format(new Date(receipt.issued_at))}</TableCell>
                    <TableCell className="text-sm">{receipt.remarks}</TableCell>
                    <TableCell className="text-right tabular-nums">{formatPaise(receipt.amount_paise)}</TableCell>
                    <TableCell className="text-right">
                      {receipt.pdf_ready ? (
                        <a
                          href={`/fees/receipts/${receipt.id}/pdf`}
                          className="text-sm underline-offset-4 hover:underline"
                        >
                          PDF
                        </a>
                      ) : (
                        <Badge variant="secondary">Generating…</Badge>
                      )}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </>
  );
}
