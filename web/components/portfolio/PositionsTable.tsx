import type { ReactNode } from "react";
import { Button, SectionHeading, Table, Td } from "@/components/ui";
import { fmtCurrency, fmtUnits } from "@/lib/format";
import type { AssetLot } from "@/lib/types";

/**
 * Holdings for the selected account, marked at each asset's opening price —
 * the same figure the account's balance is the sum of. Cost basis and purchase
 * date belong to the lot rather than to the line, so they sit in the row's
 * title rather than taking two more columns in a 372px drawer.
 */
export function PositionsTable({
  lots,
  onAddLot,
  addDisabled,
  basisTracked = true,
  note,
}: {
  lots: AssetLot[];
  onAddLot?: () => void;
  /** Offered but refused: there is no connection to save a lot through. */
  addDisabled?: boolean;
  /** False where a sale realises no gain, so the basis on a lot is never read. */
  basisTracked?: boolean;
  /** A line under the table saying what this kind does differently. */
  note?: ReactNode;
}) {
  const total = lots.reduce((sum, lot) => sum + lot.value, 0);
  return (
    <div>
      <SectionHeading
        className="mb-[4px]"
        action={
          onAddLot ? (
            <Button variant="ghost" onClick={onAddLot} disabled={addDisabled}>
              Add lot
            </Button>
          ) : (
            lots.length > 0 && (
              <span style={{ fontSize: 11.5, color: "color-mix(in srgb, var(--color-text) 55%, transparent)" }}>
                {fmtCurrency(total)}
              </span>
            )
          )
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
            {lots.map((lot) => (
              <tr
                key={lot.positionId}
                title={
                  basisTracked
                    ? `Basis ${fmtCurrency(lot.costBasis)} · purchased ${lot.purchaseDate}`
                    : undefined
                }
              >
                <Td>{lot.assetId}</Td>
                <Td align="right" muted>
                  {fmtUnits(lot.units)} u
                </Td>
                <Td align="right">{fmtCurrency(lot.value)}</Td>
              </tr>
            ))}
          </tbody>
        </Table>
      )}
      {note && (
        <div
          className="text-muted"
          style={{ fontSize: 11, marginTop: 5 }}
        >
          {note}
        </div>
      )}
    </div>
  );
}
