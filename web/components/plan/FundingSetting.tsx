"use client";

import { useState } from "react";
import { Select } from "@/components/ui";
import type { Account, FundingPolicySpec, WithdrawalStrategy } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { usePlanApi } from "@/lib/nav";
import {
  BRACKET_CEILINGS,
  DEFAULT_CEILING,
  STRATEGY_LABEL,
  STRATEGY_ORDER,
} from "@/lib/view/drawdown";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/**
 * "When cash runs short": the plan's funding policy (spec 20). Off, a bank
 * balance that runs dry records a shortfall; on, investments are sold in the
 * strategy's order, never touching the accounts listed under Never sell.
 * Each change saves on its own.
 */
export function FundingSetting({
  scenarioId,
  funding,
  accounts,
  onChanged,
  offline,
}: {
  scenarioId: number;
  funding: FundingPolicySpec | null;
  accounts: Account[];
  onChanged: () => void;
  offline?: boolean;
}) {
  const api = usePlanApi();
  const submit = useSubmit();
  const [open, setOpen] = useState(false);
  const investments = accounts.filter((a) => a.flavor === "Investment");

  const save = (next: FundingPolicySpec | null) =>
    submit.run(() => api.scenarios.setFunding(scenarioId, { funding: next }), onChanged);
  const on = (patch: Partial<FundingPolicySpec>): FundingPolicySpec => ({
    strategy: "TaxEfficientEarly",
    exclude_accounts: [],
    ...funding,
    ...patch,
  });
  const excluded = new Set(funding?.exclude_accounts ?? []);
  const disabled = offline || submit.busy;

  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        gap: "6px 10px",
        padding: "7px 20px",
        fontSize: 12.5,
        borderBottom: "1px solid var(--color-divider)",
      }}
    >
      <span style={{ color: MUTED }}>When cash runs short:</span>
      <Select
        aria-label="When cash runs short"
        value={funding ? "sell" : "record"}
        disabled={disabled}
        onChange={(e) => save(e.target.value === "sell" ? on({}) : null)}
        style={{ minHeight: 28, width: "auto", padding: "2px 8px" }}
      >
        <option value="sell">Sell investments using</option>
        <option value="record">Don&apos;t sell; record a shortfall</option>
      </Select>
      {funding && (
        <>
          <Select
            aria-label="Withdrawal strategy"
            value={funding.strategy}
            disabled={disabled}
            onChange={(e) => save(on({ strategy: e.target.value as WithdrawalStrategy, bracket_ceiling: undefined }))}
            style={{ minHeight: 28, width: "auto", padding: "2px 8px" }}
          >
            {STRATEGY_ORDER.map((s) => (
              <option key={s} value={s}>
                {STRATEGY_LABEL[s]}
              </option>
            ))}
          </Select>
          {funding.strategy === "BracketFilling" && (
            <Select
              aria-label="Bracket ceiling"
              value={funding.bracket_ceiling ?? DEFAULT_CEILING}
              disabled={disabled}
              onChange={(e) => save(on({ bracket_ceiling: Number(e.target.value) }))}
              style={{ minHeight: 28, width: "auto", padding: "2px 8px" }}
            >
              {BRACKET_CEILINGS.map((c) => (
                <option key={c} value={c}>
                  up to {Math.round(c * 100)}%
                </option>
              ))}
            </Select>
          )}
          {investments.length > 0 && (
            <span style={{ position: "relative" }}>
              <button
                type="button"
                aria-expanded={open}
                onClick={() => setOpen((v) => !v)}
                style={{ all: "unset", cursor: "pointer", textDecoration: "underline", color: MUTED }}
              >
                Never sell: {excluded.size === 0 ? "none" : `${excluded.size} account${excluded.size === 1 ? "" : "s"}`}
              </button>
              {open && (
                <div
                  role="group"
                  aria-label="Never sell"
                  style={{
                    position: "absolute",
                    zIndex: 20,
                    top: "100%",
                    left: 0,
                    marginTop: 4,
                    padding: 10,
                    minWidth: 200,
                    background: "var(--color-raised)",
                    border: "1px solid var(--color-divider)",
                    display: "flex",
                    flexDirection: "column",
                    gap: 6,
                  }}
                >
                  {investments.map((a) => (
                    <label key={a.id} style={{ display: "flex", gap: 8, alignItems: "center" }}>
                      <input
                        type="checkbox"
                        checked={excluded.has(a.id)}
                        disabled={disabled}
                        onChange={(e) => {
                          const next = new Set(excluded);
                          if (e.target.checked) next.add(a.id);
                          else next.delete(a.id);
                          save(on({ exclude_accounts: [...next] }));
                        }}
                      />
                      {a.name}
                    </label>
                  ))}
                </div>
              )}
            </span>
          )}
        </>
      )}
      {submit.error && (
        <span role="alert" style={{ color: "var(--color-danger)" }}>
          Not saved: {submit.error}
        </span>
      )}
    </div>
  );
}
