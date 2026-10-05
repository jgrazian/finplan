"use client";

import { useState } from "react";
import { Dialog, DialogRow, Dropdown, Field, NumberInput } from "@/components/ui";
import type { Account } from "@/lib/api/types";
import { CEILINGS, type ConversionChoice, conversionAccounts, percentFigure } from "@/lib/view/conversion";
import { Note, accountOptions } from "./TriggerFields";

/** The payer menu's entry for "take the tax out of the conversion". */
const WITHHOLD = 0;

/**
 * "Add Roth conversions": the `RothConversions` template as a short form. One
 * yearly Dec 30 event, last in the list, converting up to the top of the
 * chosen bracket from the start year until RMDs begin. Everything it sets is
 * an ordinary event the editor opens on afterwards.
 */
export function RothConversionDialog({
  accounts,
  initial,
  busy,
  error,
  onClose,
  onSubmit,
}: {
  accounts: Account[];
  initial: ConversionChoice;
  busy?: boolean;
  error?: string;
  onClose: () => void;
  onSubmit: (choice: ConversionChoice) => void;
}) {
  const [choice, setChoice] = useState(initial);
  const set = (patch: Partial<ConversionChoice>) => setChoice((c) => ({ ...c, ...patch }));
  const { from, to, payers } = conversionAccounts(accounts);

  return (
    <Dialog
      title="Add Roth conversions"
      onClose={onClose}
      onSubmit={() => onSubmit(choice)}
      submitLabel="Add event"
      busy={busy}
      error={error}
      width={560}
    >
      <p style={{ margin: 0, fontSize: 13 }}>
        Each year on Dec 30, convert pre-tax money to the Roth up to the top of a tax bracket:
        the low brackets a retirement on savings leaves unused, filled before RMDs force the
        money out at higher rates.
      </p>
      <DialogRow>
        <Field label="Convert from">
          <Dropdown
            className="dd-field"
            options={accountOptions(from)}
            value={choice.fromAccountId}
            ariaLabel="Convert from"
            onChange={(fromAccountId) => set({ fromAccountId })}
          />
        </Field>
        <Field label="Into">
          <Dropdown
            className="dd-field"
            options={accountOptions(to)}
            value={choice.toAccountId}
            ariaLabel="Convert into"
            onChange={(toAccountId) => set({ toAccountId })}
          />
        </Field>
      </DialogRow>
      <DialogRow>
        <Field label="Up to the top of">
          <Dropdown
            className="dd-field"
            options={CEILINGS.map((rate) => ({ value: rate, label: `the ${percentFigure(rate)}% bracket` }))}
            value={choice.ceilingRate}
            ariaLabel="Bracket ceiling"
            onChange={(ceilingRate) => set({ ceilingRate })}
          />
        </Field>
        <Field label="Tax paid from">
          <Dropdown
            className="dd-field"
            options={[
              { value: WITHHOLD, label: "withheld from the conversion" },
              ...accountOptions(payers),
            ]}
            value={choice.payTaxFromAccountId ?? WITHHOLD}
            ariaLabel="Tax paid from"
            onChange={(id) => set({ payTaxFromAccountId: id === WITHHOLD ? null : id })}
          />
        </Field>
      </DialogRow>
      <DialogRow>
        <Field label="From the year">
          <NumberInput
            value={choice.startYear}
            onValueChange={(startYear) => set({ startYear })}
            min={1900}
            max={2200}
            aria-label="First year"
          />
        </Field>
        <Field label="Until age">
          <NumberInput
            value={choice.untilAge}
            onValueChange={(untilAge) => set({ untilAge })}
            min={0}
            max={120}
            aria-label="Until age"
          />
        </Field>
      </DialogRow>
      <Note>
        {choice.payTaxFromAccountId == null
          ? "Withheld, less reaches the Roth, and before 59½ the withheld part pays the 10% early-withdrawal penalty."
          : "Paying the tax from another account keeps the whole conversion in the Roth."}{" "}
        Conversions stop at 73, when RMDs begin; the last is the year before. Before 59½ each
        conversion must stay five years in the Roth before it can be withdrawn without penalty.
      </Note>
      <Note>
        Not modeled: IRMAA Medicare surcharges, ACA premium credits, the taxable share of Social
        Security, and long-term gains stacking on ordinary income.
      </Note>
    </Dialog>
  );
}
