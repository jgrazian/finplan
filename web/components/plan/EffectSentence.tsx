"use client";

import { useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import {
  Blueprint,
  Button,
  CurrencyInput,
  DragHandle,
  DropLine,
  Dropdown,
  type DropdownOption,
  Field,
  NumberInput,
  SectionHeading,
} from "@/components/ui";
import { monthlyPayment } from "@/components/portfolio/AccountTerms";
import { useReorder } from "@/lib/hooks/useReorder";
import { money } from "@/lib/view/format";
import { describeEffect, namesOf } from "@/lib/view/events";
import { AmountExpression } from "./AmountFields";
import { readStaticAmount, withRootAmountMode } from "./amountDraft";
import { Note, type TriggerContext, accountOptions, assetOptions, eventOptions } from "./TriggerFields";
import {
  AMOUNT_MODES,
  EFFECT_FORMS,
  FAMILY,
  type EffectDraft,
  type EffectForm,
  LOT_METHODS,
  STRATEGIES,
  VERBS,
  type Verb,
  collapseAmount,
  effectConversion,
  collapseDownPayment,
  downPaymentProbe,
  emptyEffect,
  expandAmount,
  expandDownPayment,
  shape,
  toEffectSpec,
  updateExpression,
} from "./effectDraft";

const MUTED = "color-mix(in srgb, var(--color-text) 55%, transparent)";

/** Hanging indent of a card's sentence: its grip and number, plus the gap. */
const CARD_HANG = 36;

const KIND_OPTIONS: DropdownOption<EffectForm>[] = EFFECT_FORMS.map((f) => ({
  value: f,
  label: f,
  detail: FAMILY[f].label,
}));

const VERB_OPTIONS: DropdownOption<Verb>[] = VERBS.map((v) => ({
  value: v,
  label: v.replace("Event", "").toLowerCase(),
}));

const INCOME_TYPES: DropdownOption<string>[] = [
  { value: "Taxable", label: "Taxable" },
  { value: "TaxFree", label: "Tax-free" },
];

const GROWTH: DropdownOption<string>[] = [
  { value: "inflation", label: "with inflation" },
  { value: "none", label: "none" },
];

/** The sentinel row standing for "no particular holding" in the asset menu. */
const ANY_ASSET = "any";

/**
 * Artboard 17a · What — the effects stacked as sentence cards.
 *
 * Each effect is one line that reads the way the money moves: "Income of
 * $8,750 into USAA as Taxable", then "AssetPurchase $3,937 of JLGMX from USAA
 * into Fidelity 401(k)". Every value in the line is editable where it stands.
 * Cards start collapsed. One at a time can be opened, and only that one shows the terms that are
 * not part of the sentence — gross or net, growth, lots, the full expression
 * editor — so the Selected Effect panel the old column needed goes away.
 */
export function EffectCards({
  effects,
  context,
  onChange,
  disabled,
}: {
  effects: EffectDraft[];
  context: TriggerContext;
  onChange: (next: EffectDraft[]) => void;
  disabled?: boolean;
}) {
  const byUid = new Map(effects.map((e) => [e.uid, e]));
  // Every card starts as its one-line sentence; only one added here opens.
  const [open, setOpen] = useState<number>();
  // Which of the open card's expressions has its editor showing — at most one,
  // so a purchase's price and down payment never stack two editors.
  const [editing, setEditing] = useState<ExprSlot>();
  const toggle = (uid: number | undefined) => {
    setOpen(uid);
    setEditing(undefined);
  };
  const edit = (uid: number, slot: ExprSlot | undefined) => {
    setOpen(uid);
    setEditing(slot);
  };

  const reorder = (order: number[]) =>
    onChange(order.map((uid) => byUid.get(uid)).filter((e): e is EffectDraft => e != null));
  const { order, attachList, attachRow, dragging, indicator, handleProps, listStyle } =
    useReorder({ keys: effects.map((e) => e.uid), onReorder: reorder, disabled });

  const add = () => {
    const fresh = emptyEffect(context.accounts[0]?.id ?? 0, context.assets[0]?.id ?? 0);
    onChange([...effects, fresh]);
    toggle(fresh.uid);
  };
  const remove = (uid: number) => onChange(effects.filter((e) => e.uid !== uid));
  const patch = (uid: number, change: Partial<EffectDraft>) =>
    onChange(effects.map((e) => (e.uid === uid ? { ...e, ...change } : e)));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
      <SectionHeading
        action={
          <Button variant="ghost" disabled={disabled} onClick={add}>
            Add effect
          </Button>
        }
      >
        What{" "}
        {effects.length > 0 && (
          <span className="text-muted" style={{ fontWeight: 400 }}>
            {effects.length}
            {effects.length > 1 && " · applied in order, drag to change"}
          </span>
        )}
      </SectionHeading>

      {effects.length === 0 ? (
        <Note>
          Nothing happens when this fires. An event with no effects still counts against a
          repeat limit, but moves no money.
        </Note>
      ) : (
        <div ref={attachList} style={{ ...listStyle, display: "flex", flexDirection: "column", gap: 10 }}>
          <DropLine at={indicator} />
          {order.map((uid, index) => {
            const effect = byUid.get(uid);
            if (!effect) return null;
            const expanded = open === uid;
            return (
              <div key={uid} ref={attachRow(uid)} className="griprow" style={{ opacity: dragging === uid ? 0.5 : undefined }}>
                <Blueprint
                  style={{
                    padding: expanded ? "14px 16px" : "12px 16px",
                    display: "flex",
                    flexDirection: "column",
                    gap: 14,
                    boxShadow: expanded ? "inset 3px 0 0 var(--color-accent)" : undefined,
                  }}
                >
                  <div className="sentence" style={{ "--hang": `${CARD_HANG}px` } as CSSProperties}>
                    {/* The lead hangs in the indent, so a wrapped sentence
                        continues under the effect's kind, not under the grip. */}
                    <span
                      style={{
                        display: "inline-flex",
                        alignItems: "center",
                        gap: 8,
                        width: CARD_HANG - 8,
                      }}
                    >
                      <DragHandle label={`effect ${index + 1}`} props={handleProps(uid)} />
                      <span className="stat-l">{index + 1}</span>
                    </span>
                    <EffectSlots
                      effect={effect}
                      context={context}
                      disabled={disabled}
                      onChange={(change) => patch(uid, change)}
                      editing={expanded ? editing : undefined}
                      onEdit={(slot) => edit(uid, slot)}
                    />
                    <span style={{ marginLeft: "auto", display: "flex", gap: 4 }}>
                      {expanded && (
                        <Button variant="ghost" style={{ fontSize: 12 }} disabled={disabled} onClick={() => remove(uid)}>
                          Remove
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        style={{ fontSize: 12 }}
                        aria-expanded={expanded}
                        onClick={() => toggle(expanded ? undefined : uid)}
                      >
                        {expanded ? "Less" : "More"}
                      </Button>
                    </span>
                  </div>
                  {expanded && (
                    <EffectDetails
                      // A fresh mount per effect, so the conversion note
                      // names the kind this effect arrived as.
                      key={uid}
                      effect={effect}
                      context={context}
                      disabled={disabled}
                      onChange={(change) => patch(uid, change)}
                      editing={editing}
                      onEdit={(slot) => edit(uid, slot)}
                    />
                  )}
                </Blueprint>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/**
 * The effect as a line: kind, then its terms in the order the money moves.
 *
 * The amount slot is the one that can be code: `ƒ` beside it flips the figure
 * into an expression and back, and an expression shows as its source in the
 * line — its editor lives in the open card, where there is room for it.
 */
function EffectSlots({
  effect,
  context,
  onChange,
  editing,
  onEdit,
  disabled,
}: {
  effect: EffectDraft;
  context: TriggerContext;
  onChange: (change: Partial<EffectDraft>) => void;
  /** The expression whose editor is showing, if any. */
  editing?: ExprSlot;
  /** Show one expression's editor, or none. Opens the card. */
  onEdit: (slot: ExprSlot | undefined) => void;
  disabled?: boolean;
}) {
  const names = namesOf(context);
  const { accounts, assets, events } = context;

  if (effect.raw) {
    return (
      <>
        <span className="slot">{effect.raw.kind}</span>
        <span>{describeEffect(effect.raw, names).detail}</span>
      </>
    );
  }

  const fields = shape(effect.form);
  const kind = (
    <Dropdown
      inline
      options={KIND_OPTIONS}
      value={effect.form}
      ariaLabel="Effect"
      disabled={disabled}
      maxMenuHeight={320}
      onChange={(form) => onChange({ form })}
    />
  );

  const account = (key: "fromAccountId" | "toAccountId", label: string) => (
    <Dropdown
      inline
      options={accountOptions(accounts)}
      value={effect[key]}
      placeholder={accounts.length === 0 ? "no accounts" : "pick an account"}
      ariaLabel={label}
      disabled={disabled}
      maxMenuHeight={300}
      onChange={(id) => onChange({ [key]: id })}
    />
  );

  // The two property effects pick from the accounts of the right kind only:
  // a mortgage offered as the house, or a house as the payer, is a mistake the
  // menu can simply not allow.
  const property = (
    <Dropdown
      inline
      options={accountOptions(accounts.filter((a) => a.flavor === "Property"))}
      value={effect.propertyAccountId}
      placeholder="pick a property"
      ariaLabel="Property"
      disabled={disabled}
      maxMenuHeight={300}
      onChange={(propertyAccountId) => onChange({ propertyAccountId })}
    />
  );
  const cashAccount = (key: "fromAccountId" | "toAccountId", label: string) => (
    <Dropdown
      inline
      options={accountOptions(accounts.filter((a) => a.flavor === "Bank" || a.flavor === "Investment"))}
      value={effect[key]}
      placeholder="pick an account"
      ariaLabel={label}
      disabled={disabled}
      maxMenuHeight={300}
      onChange={(id) => onChange({ [key]: id })}
    />
  );

  const asset = (
    <Dropdown<string | number>
      inline
      options={[...(fields.anyAsset ? [{ value: ANY_ASSET, label: "any holding" }] : []), ...assetOptions(assets)]}
      value={effect.anyAsset ? ANY_ASSET : effect.assetId}
      placeholder={assets.length === 0 ? "no assets" : "pick an asset"}
      ariaLabel="Asset"
      disabled={disabled}
      maxMenuHeight={300}
      onChange={(picked) =>
        onChange(picked === ANY_ASSET ? { anyAsset: true } : { anyAsset: false, assetId: Number(picked) })
      }
    />
  );

  const expression = effect.rawAmount?.kind === "Expression" ? effect.rawAmount.source : null;
  const amount = (
    <>
      {effect.rawAmount ? (
        <button
          type="button"
          className={editing === "amount" ? "slot x active" : "slot x"}
          title={expression ?? undefined}
          onClick={() => onEdit("amount")}
        >
          {expression || <span style={{ color: MUTED }}>expression</span>}
        </button>
      ) : (
        <CurrencyInput
          value={effect.amount}
          readOnly={disabled}
          allowNegative={effect.form === "AdjustBalance"}
          onValueChange={(value) => onChange({ amount: value, expressionDraft: undefined })}
          aria-label="Amount per occurrence"
        />
      )}
      <FxButton
        slot="amount"
        isExpression={!!effect.rawAmount}
        editing={editing}
        disabled={disabled}
        onExpand={() => onChange(expandAmount(effect, names))}
        onEdit={onEdit}
      />
    </>
  );

  switch (effect.form) {
    case "Income":
      return (
        <>
          {kind}<span>of</span>{amount}<span>into</span>{account("toAccountId", "To account")}<span>as</span>
          <Dropdown
            inline
            options={INCOME_TYPES}
            value={effect.taxFree ? "TaxFree" : "Taxable"}
            ariaLabel="Income type"
            disabled={disabled}
            onChange={(picked) => onChange({ taxFree: picked === "TaxFree" })}
          />
        </>
      );
    case "Expense":
      return <>{kind}<span>of</span>{amount}<span>from</span>{account("fromAccountId", "From account")}</>;
    case "CashTransfer":
      return (
        <>
          {kind}<span>of</span>{amount}<span>from</span>{account("fromAccountId", "From account")}
          <span>into</span>{account("toAccountId", "To account")}
        </>
      );
    case "AssetPurchase":
      return (
        <>
          {kind}{amount}<span>of</span>{asset}<span>from</span>{account("fromAccountId", "From account")}
          <span>into</span>{account("toAccountId", "To account")}
        </>
      );
    case "AssetSale":
      return <>{kind}{amount}<span>of</span>{asset}<span>from</span>{account("fromAccountId", "From account")}</>;
    case "Sweep":
      return <>{kind}{amount}<span>into</span>{account("toAccountId", "To account")}</>;
    case "AdjustBalance":
      return <>{kind}{account("toAccountId", "Account")}<span>by</span>{amount}</>;
    case "ApplyRmd":
      return <>{kind}<span>into</span>{account("toAccountId", "To account")}</>;
    case "RsuVesting":
      return (
        <>
          {kind}
          <NumberInput
            value={effect.units}
            readOnly={disabled}
            onValueChange={(units) => onChange({ units })}
            min={0}
            suffix="units"
            aria-label="Units"
          />
          <span>of</span>{asset}<span>into</span>{account("toAccountId", "To account")}
        </>
      );
    case "DeleteAccount":
      return <>{kind}{account("toAccountId", "Account")}</>;
    case "BuyProperty": {
      const loans = accounts.filter((a) => a.flavor === "Liability");
      return (
        <>
          {kind}
          {property}
          <span>for</span>
          {amount}
          <span>from</span>
          {cashAccount("fromAccountId", "Paid from")}
          <span>financed by</span>
          <Dropdown
            inline
            options={[
              { value: 0, label: "no loan (cash)" },
              ...loans.map((a) => ({ value: a.id, label: a.name })),
            ]}
            value={effect.financed ? effect.loanAccountId : 0}
            placeholder="pick a loan"
            ariaLabel="Loan"
            disabled={disabled}
            onChange={(id) => onChange(id === 0 ? { financed: false } : { financed: true, loanAccountId: id })}
          />
          {effect.financed && (
            <>
              <span>with</span>
              {effect.rawDownPayment ? (
                <button
                  type="button"
                  className={editing === "down" ? "slot x active" : "slot x"}
                  title={effect.rawDownPayment.kind === "Expression" ? effect.rawDownPayment.source : undefined}
                  onClick={() => onEdit("down")}
                >
                  {effect.rawDownPayment.kind === "Expression" && effect.rawDownPayment.source
                    ? effect.rawDownPayment.source
                    : <span style={{ color: MUTED }}>expression</span>}
                </button>
              ) : (
                <CurrencyInput
                  value={effect.downPayment}
                  readOnly={disabled}
                  onValueChange={(downPayment) => onChange({ downPayment })}
                  aria-label="Down payment"
                />
              )}
              <FxButton
                slot="down"
                isExpression={!!effect.rawDownPayment}
                editing={editing}
                disabled={disabled}
                onExpand={() => onChange(expandDownPayment(effect))}
                onEdit={onEdit}
              />
              <span>down over</span>
              <NumberInput
                value={effect.termMonths}
                readOnly={disabled}
                onValueChange={(m) => onChange({ termMonths: Math.max(1, Math.round(m)) })}
                decimals={0}
                min={1}
                max={600}
                suffix="months"
                aria-label="Term in months"
              />
            </>
          )}
        </>
      );
    }
    case "SellProperty": {
      const loans = accounts.filter((a) => a.flavor === "Liability");
      return (
        <>
          {kind}
          {property}
          <span>into</span>
          {cashAccount("toAccountId", "Proceeds to")}
          <span>less</span>
          <NumberInput
            value={effect.sellingCostRate * 100}
            readOnly={disabled}
            onValueChange={(pct) => onChange({ sellingCostRate: Math.min(100, Math.max(0, pct)) / 100 })}
            decimals={2}
            min={0}
            max={100}
            suffix="%"
            aria-label="Selling costs"
          />
          <span>costs, paying off</span>
          <Dropdown
            inline
            options={[
              { value: 0, label: "no loan" },
              ...loans.map((a) => ({ value: a.id, label: a.name })),
            ]}
            value={effect.payoff ? effect.loanAccountId : 0}
            placeholder="pick a loan"
            ariaLabel="Loan paid off"
            disabled={disabled}
            onChange={(id) => onChange(id === 0 ? { payoff: false } : { payoff: true, loanAccountId: id })}
          />
        </>
      );
    }
    case "MarketShock":
      return (
        <>
          {kind}
          <span>: markets fall</span>
          <NumberInput
            value={Number((effect.drop * 100).toFixed(2))}
            readOnly={disabled}
            onValueChange={(pct) => onChange({ drop: Math.min(99, Math.max(0, pct)) / 100 })}
            decimals={2}
            min={0}
            max={99}
            suffix="%"
            aria-label="Market drop"
          />
        </>
      );
    case "Event control":
      return (
        <>
          {kind}
          <Dropdown
            inline
            options={VERB_OPTIONS}
            value={effect.verb}
            ariaLabel="Verb"
            disabled={disabled}
            onChange={(verb) => onChange({ verb })}
          />
          <Dropdown
            inline
            options={eventOptions(events, context.selfId)}
            value={effect.targetEventId}
            placeholder="pick an event"
            ariaLabel="Target event"
            disabled={disabled}
            maxMenuHeight={300}
            onChange={(targetEventId) => onChange({ targetEventId })}
          />
        </>
      );
  }
}

/** The open card's second tier: what is not part of the sentence. */
function EffectDetails({
  effect,
  context,
  onChange,
  editing,
  onEdit,
  disabled,
}: {
  effect: EffectDraft;
  context: TriggerContext;
  onChange: (change: Partial<EffectDraft>) => void;
  editing?: ExprSlot;
  onEdit: (slot: ExprSlot | undefined) => void;
  disabled?: boolean;
}) {
  const [was] = useState<EffectForm>(effect.form);

  if (effect.raw) {
    return <Note>Built outside this form and saved back untouched.</Note>;
  }

  const fields = shape(effect.form);
  const conversion = effectConversion(was, effect.form);
  const grid: ReactNode[] = [];

  if (fields.mode) {
    grid.push(
      <Field key="mode" label="Amount is">
        <Dropdown
          className="dd-field"
          options={AMOUNT_MODES}
          value={effect.amountMode}
          ariaLabel="Amount is"
          disabled={disabled}
          onChange={(amountMode) =>
            onChange({
              amountMode,
              ...(effect.rawAmount?.kind === "Expression"
                ? updateExpression(effect, withRootAmountMode(effect.rawAmount.source, amountMode))
                : {}),
            })
          }
        />
      </Field>,
    );
  }
  // Growth stands in for the "in today's money" tick. An expression says its
  // own growth with inflation(), so the field only exists for a plain figure.
  if (fields.amount && !effect.rawAmount) {
    grid.push(
      <Field key="growth" label="Growth">
        <Dropdown
          className="dd-field"
          options={GROWTH}
          value={effect.inflationAdjusted ? "inflation" : "none"}
          ariaLabel="Growth"
          disabled={disabled}
          onChange={(g) => onChange({ inflationAdjusted: g === "inflation", expressionDraft: undefined })}
        />
      </Field>,
    );
  }
  if (fields.strategy && !effect.rawSources) {
    grid.push(
      <Field key="strategy" label="Source order">
        <Dropdown
          className="dd-field"
          options={STRATEGIES.map((s) => ({ value: s, label: s }))}
          value={effect.strategy}
          ariaLabel="Source order"
          disabled={disabled}
          onChange={(strategy) => onChange({ strategy })}
        />
      </Field>,
    );
  }
  if (fields.lots) {
    grid.push(
      <Field key="lots" label="Sell lots">
        <Dropdown
          className="dd-field"
          options={LOT_METHODS.map((m) => ({ value: m, label: m }))}
          value={effect.lotMethod}
          ariaLabel="Sell lots"
          disabled={disabled}
          onChange={(lotMethod) => onChange({ lotMethod })}
        />
      </Field>,
    );
  }
  // Income carries its type in the sentence; a sweep's proceeds need one too.
  if (fields.taxable && effect.form !== "Income") {
    grid.push(
      <Field key="taxable" label="Income type">
        <Dropdown
          className="dd-field"
          options={INCOME_TYPES}
          value={effect.taxFree ? "TaxFree" : "Taxable"}
          ariaLabel="Income type"
          disabled={disabled}
          onChange={(picked) => onChange({ taxFree: picked === "TaxFree" })}
        />
      </Field>,
    );
  }

  return (
    <>
      {conversion && <Note>{conversion}</Note>}
      {fields.amount && effect.rawAmount && editing === "amount" && (
        <>
          <AmountExpression
            label={effect.form === "BuyProperty" ? "Price expression" : "Value expression"}
            source={effect.rawAmount.kind === "Expression" ? effect.rawAmount.source : ""}
            effect={toEffectSpec(effect)}
            context={context}
            disabled={disabled}
            onChange={(source) => onChange(updateExpression(effect, source))}
          />
          <ExpressionActions
            onDone={() => onEdit(undefined)}
            onFigure={
              readStaticAmount(effect.rawAmount)
                ? () => {
                    onChange(collapseAmount(effect));
                    onEdit(undefined);
                  }
                : undefined
            }
            disabled={disabled}
          />
        </>
      )}
      {effect.form === "BuyProperty" && effect.financed && effect.rawDownPayment?.kind === "Expression" &&
        editing === "down" && (
          <>
            <AmountExpression
              label="Down payment expression"
              source={effect.rawDownPayment.source}
              effect={downPaymentProbe(effect)}
              context={context}
              disabled={disabled}
              onChange={(source) => onChange({ rawDownPayment: { kind: "Expression", source } })}
            />
            <ExpressionActions
              onDone={() => onEdit(undefined)}
              onFigure={
                collapseDownPayment(effect)
                  ? () => {
                      onChange(collapseDownPayment(effect) ?? {});
                      onEdit(undefined);
                    }
                  : undefined
              }
              disabled={disabled}
            />
          </>
        )}
      {(grid.length > 0 || fields.units) && <Tier>
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))",
            gap: 12,
            flex: 1,
            minWidth: 0,
          }}
        >
          {grid}
        </div>
        <div style={{ display: "flex", alignItems: "flex-end", gap: 14, flex: "none" }}>
          {fields.units && (
            <label className="radio" style={{ fontSize: 12 }}>
              <input
                type="checkbox"
                checked={effect.sellToCover}
                disabled={disabled}
                onChange={(e) => onChange({ sellToCover: e.target.checked })}
              />
              <span className="dot" />
              Sell to cover withholding
            </label>
          )}
        </div>
      </Tier>}
      {fields.strategy && effect.rawSources && (
        <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
          <Note>Draws from a named source list this form cannot draw, kept as saved.</Note>
          <Button variant="ghost" disabled={disabled} onClick={() => onChange({ rawSources: undefined })}>
            Use a source order
          </Button>
        </div>
      )}
      {effect.form === "ApplyRmd" && (
        <Note>
          Amount is set by the IRS uniform lifetime table at the age this fires, so there is no
          figure to type.
        </Note>
      )}
      {effect.form === "AdjustBalance" && (
        <Note>
          Writes the balance with no counterparty — nothing is sold and no tax is computed.
          Negative amounts reduce it.
        </Note>
      )}
      {effect.form === "BuyProperty" && <PurchaseNote effect={effect} context={context} />}
      {effect.form === "SellProperty" && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(170px, 1fr))", gap: 12 }}>
          <Field label="Gain excluded from tax">
            <Dropdown
              className="dd-field"
              options={[
                { value: 0, label: "none" },
                { value: 250_000, label: "$250,000", detail: "single" },
                { value: 500_000, label: "$500,000", detail: "joint" },
                ...([0, 250_000, 500_000].includes(effect.gainExclusion)
                  ? []
                  : [{ value: effect.gainExclusion, label: `$${effect.gainExclusion.toLocaleString("en-US")}` }]),
              ]}
              value={effect.gainExclusion}
              ariaLabel="Gain excluded from tax"
              disabled={disabled}
              onChange={(gainExclusion) => onChange({ gainExclusion })}
            />
          </Field>
          <Note>
            A primary residence lived in two of the last five years excludes $250,000 of gain, or
            $500,000 filing jointly. The rest is taxed as a long-term gain over the purchase price.
          </Note>
        </div>
      )}
      {effect.form === "DeleteAccount" && (
        <Note>
          Removes the account from the plan when this fires, with whatever it still holds — put
          an AssetSale or CashTransfer above this effect to move the value out first.
        </Note>
      )}
      {fields.verb && (
        <Note>Terminate is irreversible within a run; Pause can be resumed by a later effect.</Note>
      )}
    </>
  );
}

/** The hairline-topped row under the sentence. */
function Tier({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "flex-end",
        gap: 14,
        paddingTop: 12,
        borderTop: "1px solid var(--color-divider)",
      }}
    >
      {children}
    </div>
  );
}

/**
 * What the purchase commits to each month, worked out the way the engine will:
 * the loan's own rate on price less down payment, over the term. And the one
 * thing a purchase does not do — carry the home — said with the expense that
 * does it.
 */
function PurchaseNote({ effect, context }: { effect: EffectDraft; context: TriggerContext }) {
  const loan = context.accounts.find((a) => a.id === effect.loanAccountId);
  const house = context.accounts.find((a) => a.id === effect.propertyAccountId);
  const borrowed = effect.rawAmount || effect.rawDownPayment ? null : effect.amount - effect.downPayment;
  return (
    <Note>
      {effect.financed && loan?.flavor === "Liability" && borrowed != null && borrowed > 0 && (
        <>
          Borrows {money(borrowed)} at {Number((loan.interest_rate * 100).toFixed(3))}% — the{" "}
          {loan.name} account&rsquo;s rate — and pays{" "}
          <strong style={{ fontWeight: 600 }}>{money(monthlyPayment(borrowed, loan.interest_rate, effect.termMonths))}/mo</strong>{" "}
          from the month after purchase until it is paid off.{" "}
        </>
      )}
      For property tax, insurance and upkeep, add a monthly Expense of{" "}
      <span className="cd-name">0.001 * balance(&quot;{house?.name ?? "House"}&quot;)</span> — 1.2% a
      year of what the home is worth — starting when this fires.
    </Note>
  );
}

/** The two expressions a card can hold; a purchase has both. */
type ExprSlot = "amount" | "down";

/**
 * `ƒ` beside a value slot. Off, it turns the figure into an expression and
 * opens its editor; on, it shows or hides that editor. The one being edited
 * goes dark, so with two expressions in a sentence it says which is open.
 */
function FxButton({
  slot,
  isExpression,
  editing,
  disabled,
  onExpand,
  onEdit,
}: {
  slot: ExprSlot;
  isExpression: boolean;
  editing?: ExprSlot;
  disabled?: boolean;
  onExpand: () => void;
  onEdit: (slot: ExprSlot | undefined) => void;
}) {
  const active = editing === slot;
  return (
    <button
      type="button"
      className={["fx", isExpression && "on", active && "active"].filter(Boolean).join(" ")}
      aria-pressed={active}
      title={!isExpression ? "Write this as an expression" : active ? "Close the editor" : "Edit the expression"}
      disabled={disabled && !isExpression}
      onClick={() => {
        if (!isExpression) {
          onExpand();
          onEdit(slot);
        } else onEdit(active ? undefined : slot);
      }}
    >
      ƒ
    </button>
  );
}

/** Under an expression editor: close it, or — while it is still one number — go back to a figure. */
function ExpressionActions({
  onDone,
  onFigure,
  disabled,
}: {
  onDone: () => void;
  onFigure?: () => void;
  disabled?: boolean;
}) {
  return (
    <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: -6 }}>
      <Button variant="secondary" onClick={onDone}>
        Done
      </Button>
      {onFigure && (
        <Button variant="ghost" disabled={disabled} onClick={onFigure} style={{ fontSize: 12 }}>
          Use a plain figure
        </Button>
      )}
    </div>
  );
}
