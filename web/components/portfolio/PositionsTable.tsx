import { Button, SectionHeading, Table, Td } from "@/components/ui";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { AssetLot } from "@/lib/types";

/** Cost-basis lots for the selected account. */
export function PositionsTable({
  lots,
  onAddLot,
}: {
  lots: AssetLot[];
  onAddLot?: () => void;
}) {
  return (
    <div>
      <SectionHeading
        className="mb-[4px]"
        action={
          <Button variant="ghost" onClick={onAddLot}>
            Add lot
          </Button>
        }
      >
        Positions
      </SectionHeading>
      {lots.length === 0 ? (
        <p className="text-muted" style={{ fontSize: 12, margin: "6px 0 0" }}>
          No lots — this account holds no tracked positions.
        </p>
      ) : (
        <Table compact>
          <tbody>
            {lots.map((lot, i) => (
              <tr key={`${lot.assetId}-${i}`}>
                <Td>{lot.assetId}</Td>
                <Td align="right">{fmtUnits(lot.units)} sh</Td>
                <Td align="right">{fmtCurrency(lot.costBasis)}</Td>
              </tr>
            ))}
          </tbody>
        </Table>
      )}
    </div>
  );
}
