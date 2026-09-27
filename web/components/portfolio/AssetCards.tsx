"use client";

import { fmtCurrency } from "@/lib/format";
import type { AssetMix, AssetRow } from "@/lib/view/assets";

/**
 * The holdings on a phone: one card per asset instead of the nine-column
 * table. Ticker and name on top, the profile under them, the value against
 * the right margin. The swatch is the profile's colour in the mix above, as
 * the share bar is on desktop. No grip and no bulk-select box — reordering
 * and remapping several at once stay desktop affairs.
 */
export function AssetCards({
  rows,
  mix,
  selectedId,
  onSelect,
}: {
  rows: AssetRow[];
  mix: AssetMix;
  selectedId: number | undefined;
  onSelect: (row: AssetRow) => void;
}) {
  return (
    <ul className="row-cards" aria-label="Assets">
      {rows.map((row) => (
        <li key={row.serverId}>
          <button
            type="button"
            className="row-card"
            aria-current={row.serverId === selectedId || undefined}
            onClick={() => onSelect(row)}
          >
            <i
              className="row-card-swatch"
              style={{ background: mix.colors.get(row.profileServerId) }}
              aria-hidden
            />
            <span className="row-card-text">
              <span className="row-card-title">
                <span className="row-card-ticker">{row.ticker}</span>
                {row.name && <span className="row-card-name">{row.name}</span>}
              </span>
              <span className="row-card-meta" data-faint={!row.profileId || undefined}>
                {row.profileId ?? "Unmapped"}
              </span>
            </span>
            {/* An unheld asset has no value yet; its price stands in, faint,
                as it does in the table. */}
            <span className="row-card-figure" data-faint={row.units === 0 || undefined}>
              {fmtCurrency(row.units === 0 ? row.price : row.value)}
            </span>
            <span className="row-card-chevron" aria-hidden>
              ›
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
