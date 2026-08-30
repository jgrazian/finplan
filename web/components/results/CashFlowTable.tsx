import { SectionHeading, Table, Td, Th } from "@/components/ui";
import { fmtCurrency } from "@/lib/format";
import type { Percentile, YearlyCashFlow } from "@/lib/types";

export function CashFlowTable({
  rows,
  percentile,
}: {
  rows: YearlyCashFlow[];
  percentile: Percentile;
}) {
  return (
    <div>
      <SectionHeading className="mb-[6px]">
        Cash flow — {percentile.toUpperCase()} run
      </SectionHeading>
      <Table>
        <thead>
          <tr>
            <Th>Year</Th>
            <Th align="right">Income</Th>
            <Th align="right">Spending</Th>
            <Th align="right">Taxes</Th>
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.year}>
              <Td>{r.year}</Td>
              <Td align="right">{fmtCurrency(r.income)}</Td>
              <Td align="right">{fmtCurrency(r.expenses)}</Td>
              <Td align="right">{fmtCurrency(r.taxes)}</Td>
            </tr>
          ))}
        </tbody>
      </Table>
    </div>
  );
}
