import type { EffectKind } from "@/lib/types";

/**
 * What an effect does to the plan's money, in five families — few enough that
 * a coloured dot can carry the family at a glance down the event rail and the
 * timeline, with the effect's own name always printed beside it.
 */
export type EffectFamily = "inflow" | "outflow" | "transfer" | "control" | "shock" | "marker";

const FAMILY: Record<EffectKind, EffectFamily> = {
  Income: "inflow",
  RsuVesting: "inflow",
  AssetSale: "inflow",
  SellProperty: "inflow",
  Expense: "outflow",
  BuyProperty: "outflow",
  CashTransfer: "transfer",
  Sweep: "transfer",
  AssetPurchase: "transfer",
  AdjustBalance: "transfer",
  ApplyRmd: "transfer",
  RothConversion: "transfer",
  PauseEvent: "control",
  ResumeEvent: "control",
  TriggerEvent: "control",
  TerminateEvent: "control",
  DeleteAccount: "control",
  Random: "control",
  MarketShock: "shock",
};

/** The family of an event's first effect, or `marker` when it has none. */
export function effectFamily(kind: EffectKind | undefined): EffectFamily {
  return kind ? FAMILY[kind] : "marker";
}

export const FAMILY_LABEL: Record<EffectFamily, string> = {
  inflow: "Money in",
  outflow: "Money out",
  transfer: "Moves money",
  control: "Controls events",
  shock: "Market shock",
  marker: "Marker",
};
