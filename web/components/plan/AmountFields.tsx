"use client";

import type { ReactNode } from "react";
import { Button, CurrencyInput, Dropdown, Field, NumberInput } from "@/components/ui";
import type { AmountSpec } from "@/lib/api/types";
import { describeAmount, namesOf } from "@/lib/view/events";
import { ratePercent } from "@/lib/view/format";
import { Note, type TriggerContext } from "./TriggerFields";
import {
  AMOUNT_HINT,
  AMOUNT_OPTIONS,
  operandLabels,
  operandsOf,
  withAmountKind,
  withOperand,
} from "./amountDraft";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/** A row of controls that wraps rather than squeezing — nodes run 1–3 wide. */
function Row({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))",
        gap: 10,
      }}
    >
      {children}
    </div>
  );
}

/**
 * The whole amount, as one expression under a heading line.
 *
 * The line is the amount in the same words the effect list uses for it, so the
 * tree below always has something to be read against — a five-node expression
 * is much easier to check against `min($40,000, 4% of Brokerage balance)` than
 * against nothing.
 */
export function AmountExpression({
  amount,
  context,
  onChange,
  onCollapse,
  disabled,
}: {
  amount: AmountSpec;
  context: TriggerContext;
  onChange: (next: AmountSpec) => void;
  /** Back to the plain currency field, where the caller offers that. */
  onCollapse?: () => void;
  disabled?: boolean;
}) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 10, flexWrap: "wrap" }}>
        <span style={{ fontSize: 12, lineHeight: 1.45, color: MUTED, minWidth: 0 }}>
          Amount: {describeAmount(amount, namesOf(context))}
        </span>
        {onCollapse && (
          <Button
            variant="ghost"
            disabled={disabled}
            style={{ marginLeft: "auto" }}
            onClick={onCollapse}
          >
            Use a static value
          </Button>
        )}
      </div>
      <AmountNode
        amount={amount}
        context={context}
        disabled={disabled}
        onChange={onChange}
      />
    </div>
  );
}

/**
 * One node of the expression, and its operands under it.
 *
 * Depth is drawn as a rule down the left rather than as indentation alone: the
 * drawer is half a page wide, and four levels of padding would leave the
 * innermost select too narrow to read the account name out of. The rule costs
 * 11px a level and still says which operand belongs to which operator.
 */
function AmountNode({
  amount,
  context,
  onChange,
  disabled,
  depth = 0,
  label = "Amount",
}: {
  amount: AmountSpec;
  context: TriggerContext;
  onChange: (next: AmountSpec) => void;
  disabled?: boolean;
  depth?: number;
  /** What this node is to its parent — "Left", "Right", "Inner". */
  label?: string;
}) {
  const { accounts, assets } = context;
  const hint = AMOUNT_HINT[amount.kind];
  const operands = operandsOf(amount);
  const labels = operandLabels(amount.kind);

  const fallback = { accountId: accounts[0]?.id ?? 0, assetId: assets[0]?.id ?? 0 };

  /**
   * Whether the kind takes terms of its own. The five operators and the two
   * pure references do not, and an empty row under them would still cost a gap.
   */
  const params =
    amount.kind === "Fixed" ||
    amount.kind === "TargetToBalance" ||
    amount.kind === "Scale" ||
    "account_id" in amount;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 8,
        ...(depth > 0
          ? { paddingLeft: 11, borderLeft: "1px solid var(--color-divider)" }
          : {}),
      }}
    >
      {/* The kind on a line of its own: a trigger and its menu share a width,
          and `Static Value (Inflation Adjusted)` in a half-column cell would
          ellipsize in the one place it has to be read against `Static Value`. */}
      <Row>
        <Field label={label}>
          <Dropdown
            className="dd-field"
            options={AMOUNT_OPTIONS}
            value={amount.kind}
            disabled={disabled}
            maxMenuHeight={300}
            onChange={(kind) => onChange(withAmountKind(amount, kind, fallback))}
            ariaLabel={label}
          />
        </Field>
      </Row>

      {params && (
        <Row>
          {(amount.kind === "Fixed" || amount.kind === "TargetToBalance") && (
            <Field label={amount.kind === "Fixed" ? "Value" : "Target"}>
              <CurrencyInput
                value={amount.value}
                readOnly={disabled}
                allowNegative
                onValueChange={(value) => onChange({ ...amount, value })}
                aria-label={amount.kind === "Fixed" ? "Value" : "Target balance"}
              />
            </Field>
          )}

          {amount.kind === "Scale" && (
            <Field label="Percentage">
              <NumberInput
                // Stored as a multiplier and typed as a percentage: 0.04 is the
                // 4% withdrawal rate anyone reading the plan would say out loud.
                value={percentOf(amount.factor)}
                readOnly={disabled}
                group={false}
                allowNegative
                suffix="%"
                onValueChange={(percent) => onChange({ ...amount, factor: factorOf(percent) })}
                aria-label="Percentage"
              />
            </Field>
          )}

          {"account_id" in amount && (
            <Field label="Account">
              <Dropdown
                className="dd-field"
                // The flavour as the detail column: "an account's cash" means
                // nothing against a Property, and the menu is where that shows.
                options={accounts.map((a) => ({
                  value: a.id,
                  label: a.name,
                  detail: a.flavor,
                }))}
                value={amount.account_id}
                // An id naming nothing — a plan with no accounts, or one deleted
                // out from under the expression — matches no row, so the trigger
                // stands empty and says why rather than showing a stale name.
                placeholder={accounts.length === 0 ? "— no accounts —" : "— pick an account —"}
                disabled={disabled}
                maxMenuHeight={300}
                onChange={(account_id) => onChange({ ...amount, account_id })}
                ariaLabel="Account"
              />
            </Field>
          )}

          {amount.kind === "AssetBalance" && (
            <Field label="Holding">
              <Dropdown
                className="dd-field"
                options={assets.map((a) => ({
                  value: a.id,
                  label: a.name,
                  detail: a.description ?? undefined,
                }))}
                value={amount.asset_id}
                placeholder={assets.length === 0 ? "— no assets —" : "— pick a holding —"}
                disabled={disabled}
                maxMenuHeight={300}
                onChange={(asset_id) => onChange({ ...amount, asset_id })}
                ariaLabel="Holding"
              />
            </Field>
          )}
        </Row>
      )}

      {hint && <Note>{hint}</Note>}

      {operands.map((child, index) => (
        <AmountNode
          key={index}
          amount={child}
          context={context}
          disabled={disabled}
          depth={depth + 1}
          label={labels[index] ?? `Operand ${index + 1}`}
          onChange={(next) => onChange(withOperand(amount, index, next))}
        />
      ))}
    </div>
  );
}

/**
 * A `Scale` factor across the percentage field and back.
 *
 * Both directions round: `0.04 * 100` is 4.000000000000001 in binary floating
 * point, and a field that showed that — or saved it back — would be its own
 * bug report.
 */
function percentOf(factor: number): number {
  return Number(ratePercent(factor).toFixed(6));
}

function factorOf(percent: number): number {
  return Number((percent / 100).toFixed(8));
}
