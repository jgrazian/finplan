"use client";

import { useState } from "react";
import type { ReactNode } from "react";
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
  SegmentedControl,
  SectionHeading,
  Tag,
} from "@/components/ui";
import { useReorder } from "@/lib/hooks/useReorder";
import { describeEffect, namesOf } from "@/lib/view/events";
import { AmountExpression } from "./AmountFields";
import { readStaticAmount, withRootAmountMode } from "./amountDraft";
import {
  Note,
  type TriggerContext,
  accountOptions,
  assetOptions,
  eventOptions,
} from "./TriggerFields";
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
  effectProblem,
  emptyEffect,
  expandAmount,
  updateExpression,
  shape,
  toEffectSpec,
} from "./effectDraft";

const MUTED = "color-mix(in srgb, var(--color-text) 58%, transparent)";

/** Every kind, with what it is for beside it — the tag, in the menu. */
const EFFECT_KIND_OPTIONS: DropdownOption<EffectForm>[] = EFFECT_FORMS.map((f) => ({
  value: f,
  label: f,
  detail: FAMILY[f].label,
}));

/** `TriggerEvent` reads as "Trigger" here — the noun is the field beside it. */
const VERB_OPTIONS: DropdownOption<Verb>[] = VERBS.map((v) => ({
  value: v,
  label: v.replace("Event", ""),
}));

const INCOME_TYPES: DropdownOption<string>[] = [
  { value: "Taxable", label: "Taxable" },
  { value: "TaxFree", label: "Tax-free" },
];

/** The sentinel row standing for "no particular holding" in the asset menu. */
const ANY_ASSET = "any";

/** A row of controls that wraps rather than squeezing — terms run 1–4 wide. */
function Row({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "repeat(auto-fit, minmax(148px, 1fr))",
        gap: 10,
      }}
    >
      {children}
    </div>
  );
}

/**
 * The button that changes what the field above it *is* — the way into the
 * amount expression — and, where there is something to say, a line saying it.
 *
 * Also where something the form cannot draw gets said in the words the list
 * already uses for it. Shown rather than hidden: an effect nobody can see is
 * one that gets forgotten and then wondered about when the numbers come out
 * wrong.
 */
function Aside({ children, action }: { children?: ReactNode; action?: ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
      {children != null && (
        <span style={{ fontSize: 12, lineHeight: 1.45, color: MUTED, minWidth: 0 }}>
          {children}
        </span>
      )}
      {action}
    </div>
  );
}

/** One effect as its row reads: the kind, then what it does, in a line. */
function summarize(effect: EffectDraft, names: ReturnType<typeof namesOf>): string {
  if (effect.raw) return describeEffect(effect.raw, names).detail;
  // A draft still missing its target would read as "event 0"; say so instead.
  return effectProblem(effect, 0)
    ? "not set yet"
    : describeEffect(toEffectSpec(effect), names).detail;
}

/**
 * Blocks 4 and 5 · the ordered effect list, and the terms of whichever one is
 * selected in it.
 *
 * Master-detail rather than a stack of open cards: an event with five effects
 * would otherwise be five kind selects and thirty fields in a 400px drawer,
 * and the order — which the engine applies in sequence — would be the one
 * thing you could not see at a glance.
 */
export function EffectsBlock({
  effects,
  context,
  onChange,
  disabled,
  label = "Effects",
}: {
  effects: EffectDraft[];
  context: TriggerContext;
  onChange: (next: EffectDraft[]) => void;
  disabled?: boolean;
  /** What the list is called where it lands — "What" in the plan editor. */
  label?: string;
}) {
  const firstAccount = context.accounts[0]?.id ?? 0;
  const firstAsset = context.assets[0]?.id ?? 0;
  const names = namesOf(context);
  const byUid = new Map(effects.map((e) => [e.uid, e]));

  // Held by uid, so reordering the list does not move the selection with the
  // index. Falls back to the first row whenever the held one has been removed.
  const [pick, setPick] = useState<number | undefined>(effects[0]?.uid);
  const selected = (pick != null && byUid.get(pick)) || effects[0];

  const reorder = (order: number[]) =>
    onChange(order.map((uid) => byUid.get(uid)).filter((e): e is EffectDraft => e != null));

  // Destructured rather than kept as one object: a `ref` prop taken off a value
  // marks the whole value as a ref to the React compiler.
  const { order, attachList, attachRow, dragging, indicator, handleProps, listStyle } =
    useReorder({ keys: effects.map((e) => e.uid), onReorder: reorder, disabled });

  const add = () => {
    const fresh = emptyEffect(firstAccount, firstAsset);
    onChange([...effects, fresh]);
    setPick(fresh.uid);
  };

  const remove = (uid: number) => {
    const at = effects.findIndex((e) => e.uid === uid);
    const rest = effects.filter((e) => e.uid !== uid);
    onChange(rest);
    // Land on the neighbour that took its place, not back at the top.
    setPick(rest[Math.min(at, rest.length - 1)]?.uid);
  };

  return (
    <>
      {/* 4 · the ordered list */}
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <SectionHeading
          action={
            <Button variant="ghost" disabled={disabled} onClick={add}>
              Add effect
            </Button>
          }
        >
          {label}{" "}
          <span className="text-muted" style={{ fontWeight: 400 }}>
            {effects.length > 0 && effects.length}
          </span>
        </SectionHeading>

        {effects.length === 0 ? (
          <Note>
            Nothing happens when this fires. An event with no effects still counts
            against a repeat limit, but moves no money.
          </Note>
        ) : (
          <>
            <div ref={attachList} style={{ ...listStyle }}>
              <DropLine at={indicator} />
              {order.map((uid, index) => {
                const effect = byUid.get(uid);
                if (!effect) return null;
                const on = effect.uid === selected?.uid;
                return (
                  <div
                    key={uid}
                    ref={attachRow(uid)}
                    className="rowsel griprow"
                    aria-selected={on}
                    onClick={() => setPick(uid)}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      gap: 7,
                      padding: "5px 6px 5px 0",
                      cursor: "pointer",
                      opacity: dragging === uid ? 0.5 : undefined,
                      borderBottom: "1px solid var(--color-divider)",
                      boxShadow: on ? "inset 2px 0 0 var(--color-accent)" : undefined,
                    }}
                  >
                    <DragHandle label={`effect ${index + 1}`} props={handleProps(uid)} />
                    <span style={{ fontSize: 11, color: MUTED, width: 10 }}>{index + 1}</span>
                    <span
                      style={{
                        fontFamily: "var(--font-heading)",
                        fontWeight: 600,
                        fontSize: 13,
                        flex: "none",
                      }}
                    >
                      {effect.raw ? effect.raw.kind : effect.form}
                    </span>
                    <span
                      style={{
                        fontSize: 11.5,
                        color: MUTED,
                        minWidth: 0,
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                      }}
                    >
                      {summarize(effect, names)}
                    </span>
                  </div>
                );
              })}
            </div>
            <Note>Applied in this order when the event fires. Drag to change it.</Note>
          </>
        )}
      </div>

      {/* 5 · the selected effect's terms */}
      {selected && (
        <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
          <SectionHeading
            action={
              <Button variant="ghost" disabled={disabled} onClick={() => remove(selected.uid)}>
                Remove
              </Button>
            }
          >
            Selected effect
          </SectionHeading>
          <EffectTerms
            // A fresh mount per effect, so the conversion note below the kind
            // select belongs to the effect actually on screen.
            key={selected.uid}
            effect={selected}
            context={context}
            disabled={disabled}
            onChange={(change) =>
              onChange(
                effects.map((e) => (e.uid === selected.uid ? { ...e, ...change } : e)),
              )
            }
          />
        </div>
      )}
    </>
  );
}

/**
 * Block 5 · one effect's terms, and only the terms that kind actually has.
 *
 * The order never changes: where the money comes from, where it goes, how much,
 * then how the engine should behave. So switching kind moves a field up or down
 * but never sideways, and a term the engine does not read for this kind is
 * absent rather than greyed out.
 */
export function EffectTerms({
  effect,
  context,
  onChange,
  disabled,
}: {
  effect: EffectDraft;
  context: TriggerContext;
  onChange: (change: Partial<EffectDraft>) => void;
  disabled?: boolean;
}) {
  const { accounts, assets, events } = context;
  const fields = shape(effect.form);
  const names = namesOf(context);
  // The kind this effect arrived as, so the note names the real conversion
  // rather than the last hop of a change made twice.
  const [was] = useState<EffectForm>(effect.form);
  const conversion = effect.raw ? null : effectConversion(was, effect.form);

  const accountSelect = (
    value: number,
    key: "fromAccountId" | "toAccountId",
    label: string,
  ) => (
    <Field label={label}>
      <Dropdown
        className="dd-field"
        options={accountOptions(accounts)}
        value={value}
        placeholder={accounts.length === 0 ? "— no accounts —" : "— pick an account —"}
        ariaLabel={label}
        disabled={disabled}
        maxMenuHeight={300}
        onChange={(id) => onChange({ [key]: id })}
      />
    </Field>
  );

  if (effect.raw) {
    return (
      <Aside>
        <strong style={{ fontWeight: 600 }}>{effect.raw.kind}</strong> ·{" "}
        {describeEffect(effect.raw, names).detail}
        <br />
        Built outside this form and saved back untouched.
      </Aside>
    );
  }

  const family = FAMILY[effect.form];

  /**
   * Gross or net. Drawn beside the figure in the simple case and under the
   * expression in the other, because an expression is a block rather than a
   * field and a select sitting alongside it would read as part of the tree.
   */
  const amountModeField = (
    <Field label="Amount is">
      <Dropdown
        className="dd-field"
        options={AMOUNT_MODES}
        value={effect.amountMode}
        ariaLabel="Amount is"
        disabled={disabled}
        onChange={(amountMode) => onChange({
          amountMode,
          ...(effect.rawAmount?.kind === "Expression"
            ? updateExpression(effect, withRootAmountMode(effect.rawAmount.source, amountMode))
            : {}),
        })}
      />
    </Field>
  );

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 9 }}>
      <Row>
        <Field label="Effect">
          <Dropdown
            className="dd-field"
            // The family in the detail column rather than as a band: the list is
            // ordered by what an effect *does*, so its four families do not fall
            // into consecutive runs and grouping would split two of them in half.
            options={EFFECT_KIND_OPTIONS}
            value={effect.form}
            ariaLabel="Effect"
            disabled={disabled}
            maxMenuHeight={320}
            onChange={(form) => onChange({ form })}
          />
        </Field>
        <div style={{ alignSelf: "end", paddingBottom: 8 }}>
          <Tag tone={family.tone}>{family.label}</Tag>
        </div>
      </Row>

      {conversion && <ConversionNote>{conversion}</ConversionNote>}

      {/* where it comes from */}
      <Row>
        {fields.strategy &&
          (effect.rawSources ? null : (
            <Field label="Source order">
              <Dropdown
                className="dd-field"
                options={STRATEGIES.map((s) => ({ value: s, label: s }))}
                value={effect.strategy}
                ariaLabel="Source order"
                disabled={disabled}
                onChange={(strategy) => onChange({ strategy })}
              />
            </Field>
          ))}
        {fields.from && accountSelect(effect.fromAccountId, "fromAccountId", "From account")}
        {fields.to && accountSelect(effect.toAccountId, "toAccountId", "To account")}
        {fields.account && accountSelect(effect.toAccountId, "toAccountId", "Account")}
        {fields.asset && (
          <Field label="Asset">
            <Dropdown<string | number>
              className="dd-field"
              options={[
                ...(fields.anyAsset ? [{ value: ANY_ASSET, label: "— any holding —" }] : []),
                ...assetOptions(assets),
              ]}
              value={effect.anyAsset ? ANY_ASSET : effect.assetId}
              placeholder={assets.length === 0 ? "— no assets —" : "— pick an asset —"}
              ariaLabel="Asset"
              disabled={disabled}
              maxMenuHeight={300}
              onChange={(picked) =>
                onChange(
                  picked === ANY_ASSET
                    ? { anyAsset: true }
                    : { anyAsset: false, assetId: Number(picked) },
                )
              }
            />
          </Field>
        )}
        {fields.units && (
          <Field label="Units">
            <NumberInput
              value={effect.units}
              readOnly={disabled}
              onValueChange={(units) => onChange({ units })}
              min={0}
              suffix="units"
              aria-label="Units"
            />
          </Field>
        )}
      </Row>

      {fields.strategy && effect.rawSources && (
        <Aside
          action={
            <Button
              variant="ghost"
              disabled={disabled}
              onClick={() => onChange({ rawSources: undefined })}
            >
              Use a source order
            </Button>
          }
        >
          Draws from a named source list this form cannot draw, kept as saved.
        </Aside>
      )}

      {/* how much */}
      {fields.amount && (
        <Field label="Value entry">
          <SegmentedControl<"Static" | "Expression">
            ariaLabel="Value entry"
            value={effect.rawAmount ? "Expression" : "Static"}
            options={[
              { value: "Static", label: "Static", disabled: disabled || (!!effect.rawAmount && !readStaticAmount(effect.rawAmount)),
                title: effect.rawAmount && !readStaticAmount(effect.rawAmount)
                  ? "Edit the formula to a literal before switching to a static value" : undefined },
              { value: "Expression", label: "Expression", disabled },
            ]}
            onChange={(mode) => onChange(mode === "Expression"
              ? expandAmount(effect, names)
              : collapseAmount(effect))}
          />
        </Field>
      )}
      {fields.amount &&
        (effect.rawAmount ? (
          <>
            <AmountExpression
              source={effect.rawAmount.kind === "Expression" ? effect.rawAmount.source : ""}
              effect={toEffectSpec(effect)}
              context={context}
              disabled={disabled}
              onChange={(source) => onChange(updateExpression(effect, source))}
            />
            {!readStaticAmount(effect.rawAmount) && (
              <Note>Edit the expression to a single number before using a static value.</Note>
            )}
            {fields.mode && <Row>{amountModeField}</Row>}
          </>
        ) : (
          <>
            <Row>
              <Field label="Amount per occurrence">
                <CurrencyInput
                  value={effect.amount}
                  readOnly={disabled}
                  onValueChange={(amount) => onChange({ amount, expressionDraft: undefined })}
                  aria-label="Amount per occurrence"
                />
              </Field>
              {fields.mode && amountModeField}
            </Row>
          </>
        ))}

      {/* the kind that takes no amount at all */}
      {effect.form === "ApplyRmd" && (
        <Note>
          Amount is set by the IRS uniform lifetime table at the age this fires, so there
          is no figure to type.
        </Note>
      )}

      {/* how the engine should behave */}
      {(fields.lots || fields.taxable) && (
        <Row>
          {fields.lots && (
            <Field label="Sell lots">
              <Dropdown
                className="dd-field"
                options={LOT_METHODS.map((m) => ({ value: m, label: m }))}
                value={effect.lotMethod}
                ariaLabel="Sell lots"
                disabled={disabled}
                onChange={(lotMethod) => onChange({ lotMethod })}
              />
            </Field>
          )}
          {fields.taxable && (
            <Field label="Income type">
              <Dropdown
                className="dd-field"
                options={INCOME_TYPES}
                value={effect.taxFree ? "TaxFree" : "Taxable"}
                ariaLabel="Income type"
                disabled={disabled}
                onChange={(picked) => onChange({ taxFree: picked === "TaxFree" })}
              />
            </Field>
          )}
        </Row>
      )}

      {/* the four that drive another event: one block, the verb is the select */}
      {fields.verb && (
        <>
          <Row>
            <Field label="Verb">
              <Dropdown
                className="dd-field"
                options={VERB_OPTIONS}
                value={effect.verb}
                ariaLabel="Verb"
                disabled={disabled}
                onChange={(verb) => onChange({ verb })}
              />
            </Field>
            <Field label="Event">
              <Dropdown
                className="dd-field"
                options={eventOptions(events, context.selfId)}
                value={effect.targetEventId}
                placeholder="— pick an event —"
                ariaLabel="Target event"
                disabled={disabled}
                maxMenuHeight={300}
                onChange={(targetEventId) => onChange({ targetEventId })}
              />
            </Field>
          </Row>
          <Note>
            Terminate is irreversible within a run; Pause can be resumed by a later effect.
          </Note>
        </>
      )}

      {fields.amount && !effect.rawAmount && (
        <label className="radio" style={{ fontSize: 12 }}>
          <input
            type="checkbox"
            checked={effect.inflationAdjusted}
            disabled={disabled}
            onChange={(e) => onChange({ inflationAdjusted: e.target.checked, expressionDraft: undefined })}
          />
          <span className="dot" />
          In today&rsquo;s money
        </label>
      )}
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

      {effect.form === "AdjustBalance" && (
        <Note>
          Writes the balance with no counterparty — nothing is sold and no tax is
          computed. Negative amounts reduce it.
        </Note>
      )}
      {effect.form === "DeleteAccount" && (
        <ConversionNote>
          Removes the account from the plan when this fires. Whatever it still holds goes
          with it — order an AssetSale or CashTransfer above this effect to move the value
          out first.
        </ConversionNote>
      )}
    </div>
  );
}

/** What a conversion abandons, or what an effect will destroy, before it runs. */
function ConversionNote({ children }: { children: ReactNode }) {
  return (
    <Blueprint
      corners={false}
      style={{
        padding: "9px 11px",
        fontSize: 11.5,
        lineHeight: 1.5,
        background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
      }}
    >
      <div style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
        <svg
          width="14"
          height="14"
          viewBox="0 0 16 16"
          fill="none"
          stroke="var(--color-accent-800)"
          strokeWidth="1.5"
          strokeLinecap="round"
          style={{ flex: "none", marginTop: 2 }}
          aria-hidden
        >
          <path d="M8 2.4 14.4 13.4H1.6z" />
          <line x1="8" y1="6.4" x2="8" y2="9.4" />
          <circle cx="8" cy="11.4" r="0.6" fill="var(--color-accent-800)" />
        </svg>
        <span>{children}</span>
      </div>
    </Blueprint>
  );
}
