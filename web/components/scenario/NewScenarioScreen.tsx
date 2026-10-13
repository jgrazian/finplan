"use client";

import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import {
  Button,
  DateInput,
  DialogRow,
  Dropdown,
  Field,
  Input,
  NumberInput,
  RangeField,
  SegmentedControl,
  type SegmentOption,
} from "@/components/ui";
import { CreateAccountLink, useGuest } from "@/components/auth/GuestContext";
import type { Entitlements } from "@/lib/api/generated/Entitlements";
import type { SetupPlan } from "@/lib/api/generated/SetupPlan";
import type { Profile, Scenario, TaxConfig, UserResponse } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { PRIVACY_PROMISE } from "@/lib/local/durability";
import { HOME_LABEL, HomeIcon } from "@/components/local/HomeIcon";
import { type PlanHome, planApiFor, planCapabilities } from "@/lib/nav";
import { addYears, money, yearsBetween } from "@/lib/view/format";
import { takeHome, type TakeHome } from "@/lib/view/takeHome";
import {
  CHOICE_EXPLAINED,
  SPENDING_CHOICES,
  type SpendingChoice,
  describeRule,
  guardrails,
  switchRule,
} from "@/lib/view/spending";
import type { SpendingRuleSpec } from "@/lib/api/types";
import { DescribeSetup } from "./DescribeSetup";
import { SetupBar } from "./SetupBar";
import {
  SetupPreview,
  type PreviewAccount,
  type PreviewEvent,
  type PreviewParameter,
} from "./SetupPreview";
import { defaultTaxConfig } from "@/lib/view/assumptions";

/** The step strip's short names, then each step's question and lead. */
const steps = [
  {
    short: "Details",
    title: "Start with the plan details",
    lead: "Your birth date and how far ahead to plan. Answer the questions in order or jump between them with the steps above, or switch to Blank to start empty with these details.",
  },
  {
    short: "Cash",
    title: "Do you have money in a checking or savings account?",
    lead: "Checking is where salary lands and spending is paid from.",
  },
  {
    short: "401(k)",
    title: "Do you have a 401(k)?",
    lead: "A 401(k) is modeled as a tax-deferred retirement account. You can refine its holdings and contribution rules after setup.",
  },
  {
    short: "Investing",
    title: "Do you have investments outside a 401(k)?",
    lead: "A brokerage account, a traditional IRA or a Roth IRA.",
  },
  {
    short: "Income",
    title: "How much do you earn and spend?",
    lead: "Enter plan-start dollars. Each amount becomes a yearly event that rises with inflation; monthly spending and your retirement age become parameters you can vary later.",
  },
  {
    short: "Saving",
    title: "Would you like to contribute part of your salary to a 401(k)?",
    lead: "Contributions come out of Salary and are invested at your stock/bond mix.",
  },
  {
    short: "Assume",
    title: "Here's what we'll assume",
    lead: "Defaults based on historical data, so you don't have to guess. Each one is an ordinary setting you can change now or later.",
  },
  {
    short: "Review",
    title: "Review your starter plan",
    lead: "Everything on the right is created together as ordinary, editable accounts, events and parameters, and the plan runs as soon as it is created.",
  },
] as const;

const EMPLOYEE_401K_DEFERRAL_LIMIT_2026 = 24_500;

// The steps whose answers first write an event, for "just added".
const INCOME_STEP = 4;
const SAVING_STEP = 5;
const ASSUME_STEP = 6;

const defaultReturnProfileIds = (profiles: Profile[]) => ({
  cash: profiles.find((profile) => profile.name === "Cash / T-Bills")?.id ?? 0,
  stocks: profiles.find((profile) => profile.name === "US Total Market")?.id ?? 0,
  bonds: profiles.find((profile) => profile.name === "US Aggregate Bonds")?.id ?? 0,
});

type SetupDraft = SetupPlan & {
  has_cash: boolean | null;
  has_401k: boolean | null;
  has_other_investments: boolean | null;
  contribute_401k: boolean;
  monthly_spending: number;
  /** Retirement spending as a percent of working-year spending. */
  retirement_spending_percent: number;
  /** Fixed, or a rule that adjusts it with the market (spec 22). */
  retirement_spending_choice: SpendingChoice;
};

/** What a plan is called until it is renamed: unique, since names are per account. */
function freshName(taken: readonly string[]): string {
  const names = new Set(taken.map((name) => name.trim().toLowerCase()));
  for (let n = 1; ; n++) {
    const name = n === 1 ? "My plan" : `My plan ${n}`;
    if (!names.has(name.toLowerCase())) return name;
  }
}

/** What the setup suggests for retirement spending. */
const SUGGESTED_CHOICE: SpendingChoice = "Guardrails";

/**
 * The rule a setup's retirement spending follows: the choice, starting at
 * the yearly amount (a rate rule at the rate that amount is of the opening
 * savings). None for Fixed or no spending.
 */
function setupRule(choice: SpendingChoice, yearly: number, savings: number): SpendingRuleSpec | null {
  if (choice === "Fixed" || !(yearly > 0)) return null;
  return switchRule(guardrails(Math.round(yearly)), choice, savings);
}

/** Where retirement spending starts: a common rule of thumb, not a forecast. */
const DEFAULT_RETIREMENT_SPENDING_PERCENT = 90;

type AmountField =
  | "duration_years"
  | "retirement_age"
  | "cash"
  | "retirement_401k"
  | "investments"
  | "stock_percent"
  | "annual_income"
  | "retirement_401k_contribution_percent"
  | "monthly_spending";

const TAX_TAGS: Record<string, PreviewAccount["tax"]> = {
  Taxable: { label: "Taxable", tone: "neutral" },
  TaxDeferred: { label: "Tax-deferred", tone: "accent" },
  TaxFree: { label: "Tax-free", tone: "accent-2" },
};

function YesNo({
  name,
  value,
  onChange,
}: {
  name: string;
  value: boolean | null;
  onChange: (value: boolean) => void;
}) {
  return (
    <SegmentedControl
      name={name}
      ariaLabel="Yes or no"
      options={[
        { value: "yes", label: "Yes" },
        { value: "no", label: "No" },
      ]}
      value={value == null ? null : value ? "yes" : "no"}
      onChange={(answer) => onChange(answer === "yes")}
    />
  );
}

/**
 * The first working year from gross salary to what is left after spending,
 * so a shortfall, and the income tax that widens it, is seen before the run.
 */
function TakeHomeLine({ year, taxName }: { year: TakeHome; taxName?: string }) {
  if (year.gross <= 0 && year.spending <= 0) return null;
  const short = year.surplus < 0;
  const row = (label: ReactNode, value: number, className = "") => (
    <div className={`ns-takehome-row ${className}`}>
      <span>{label}</span>
      <span>{money(value)}</span>
    </div>
  );
  return (
    <section className="ns-takehome" aria-label="First year take-home pay">
      {row("Salary", year.gross)}
      {year.deferral > 0 && row("401(k) contribution, pre-tax", -year.deferral)}
      {row(
        <>
          Income tax, estimated{" "}
          <span className="ns-mut">· {taxName ? `federal + state, ${taxName}` : "no taxes modeled"}</span>
        </>,
        -(year.federal + year.state),
      )}
      {row("Take-home pay", year.net, "ns-takehome-total")}
      {row("Spending", -year.spending)}
      {row(
        short ? "Shortfall each year" : "Left over each year",
        year.surplus,
        `ns-takehome-total${short ? " ns-takehome-short" : ""}`,
      )}
      {short && (
        <p role="status" className="ns-takehome-short">
          You would spend {money(-year.surplus)} a year more than you take home. Until retirement the
          plan covers the gap from your savings, and runs short if they run out.
        </p>
      )}
      <p className="ns-mut ns-takehome-note">
        First plan year. Payroll taxes (Social Security and Medicare) are not modeled.
      </p>
    </section>
  );
}

/** Strip the UI-only answers before the draft crosses the API boundary. */
function setupPlanOf(draft: SetupDraft): SetupPlan {
  const plan = { ...draft };
  const answers = plan as Partial<SetupDraft>;
  delete answers.has_cash;
  delete answers.has_401k;
  delete answers.has_other_investments;
  delete answers.contribute_401k;
  delete answers.monthly_spending;
  delete answers.retirement_spending_percent;
  delete answers.retirement_spending_choice;
  plan.retirement_401k_contribution_percent = draft.contribute_401k
    ? draft.retirement_401k_contribution_percent
    : 0;
  plan.annual_spending = draft.monthly_spending * 12;
  plan.retirement_spending = (draft.monthly_spending * 12 * draft.retirement_spending_percent) / 100;
  plan.retirement_spending_rule = setupRule(
    draft.retirement_spending_choice,
    plan.retirement_spending,
    draft.cash + draft.retirement_401k + draft.investments,
  ) ?? undefined;
  return plan;
}

interface NewScenarioProps {
  defaults: UserResponse;
  inflationProfiles: Profile[];
  taxConfigs: TaxConfig[];
  returnProfiles?: Profile[];
  /** What the account may do; `ai_drafts` null (or absent) hides Describe & upload. */
  access?: Entitlements;
  /** Where the plan will be kept. The libraries above are that home's. */
  home: PlanHome;
  /** Offered only when there is a choice, i.e. local mode is on. */
  onHomeChange?: (home: PlanHome) => void;
  /** The names already used, which a new plan's default name avoids. */
  takenNames?: readonly string[];
  onClose: () => void;
  /** Guided setup opens the plan on Results, where it is run at once; Blank opens it on Plan. */
  onCreated: (s: Scenario, nextTab: "plan" | "results") => void;
  /** A draft is opened on Review; it stays a draft until Create & run. */
  onReviewDraft?: (draft: Scenario) => void;
  /** Create & run made a draft a plan and queued its run. */
  onDraftCreated?: (created: Scenario) => void;
}

type Mode = "guided" | "describe" | "blank";

/**
 * New scenario as a full page under the header (designs 1a and 2a): Guided
 * questions with the plan assembling beside them, Blank for the details
 * alone, and, where the server has AI drafts on, Describe & upload.
 */
export function NewScenarioScreen(props: NewScenarioProps) {
  const [chosen, setMode] = useState<Mode>("guided");
  // Held here rather than in the questions, so a look at Describe & upload
  // does not send Guided back to its first step.
  const [step, setStep] = useState(0);
  const access = props.access;
  const drafts = access?.ai_drafts ?? null;
  const { guest, restricted } = useGuest();
  // Drafts are written by the server for a plan it stores, so a plan kept on
  // this device cannot have one: Describe & upload is the locked state AI
  // features show there, and Guided is where the person lands instead.
  const capabilities = planCapabilities(props.home, { account: !guest });
  const mode: Mode = !capabilities.ai && chosen === "describe" ? "guided" : chosen;
  const describable =
    capabilities.ai && access != null && drafts != null && props.onReviewDraft != null && props.onDraftCreated != null;
  // A hosted guest has no AI drafts: Describe & upload stays in the list,
  // locked, with the way to unlock it beside it.
  const lockedForGuest = capabilities.ai && restricted && drafts == null;
  const lockedForHome = !capabilities.ai && drafts != null;
  const locked = lockedForGuest || lockedForHome;
  const modeSwitch = (
    <>
      {props.onHomeChange && <HomeSwitch home={props.home} onChange={props.onHomeChange} />}
      <ModeSwitch
        mode={mode}
        describable={describable}
        locked={locked}
        lockedTitle={
          lockedForHome
            ? // The way to the account is the switch beside it, not a move.
              props.onHomeChange
              ? "Choose Cloud to describe your plan and upload statements."
              : capabilities.cloudOnlyReason
            : "Create a free account to describe your plan and upload statements."
        }
        onChange={setMode}
      />
      {lockedForGuest && (
        <span className="ns-mut">
          AI drafts and document upload need an account. <CreateAccountLink />.
        </span>
      )}
    </>
  );
  if (access && drafts && props.onReviewDraft && props.onDraftCreated && mode === "describe") {
    return (
      <DescribeSetup
        access={access}
        drafts={drafts}
        modeSwitch={modeSwitch}
        onClose={props.onClose}
        onCreated={props.onDraftCreated}
        onReviewDraft={props.onReviewDraft}
      />
    );
  }
  // Guided and Blank are one component, so the details typed in one are
  // still there in the other.
  return (
    <GuidedSetup
      {...props}
      // The answers hold ids from one home's library; another home has its own.
      key={props.home}
      blank={mode === "blank"}
      step={step}
      onStep={setStep}
      modeSwitch={modeSwitch}
    />
  );
}

/**
 * Where the new plan lives; shown only when local mode gives a choice. The
 * privacy promise is said here, on the option it belongs to, rather than as a
 * line of its own: a sentence that comes and goes with the choice would move
 * the whole bar each time it is toggled.
 */
function HomeSwitch({ home, onChange }: { home: PlanHome; onChange: (home: PlanHome) => void }) {
  const options: SegmentOption<PlanHome>[] = [
    {
      value: "local",
      label: HOME_LABEL.local,
      icon: <HomeIcon home="local" />,
      title: PRIVACY_PROMISE,
    },
    {
      value: "cloud",
      label: HOME_LABEL.cloud,
      icon: <HomeIcon home="cloud" />,
      title: "The plan is saved to your FinPlan account.",
    },
  ];
  return (
    <SegmentedControl
      name="new-scenario-home"
      ariaLabel="Where to keep it"
      options={options}
      value={home}
      onChange={onChange}
    />
  );
}

function ModeSwitch({
  mode,
  describable,
  locked,
  lockedTitle,
  onChange,
}: {
  mode: Mode;
  describable: boolean;
  /** Shown but disabled: the account would have it, a guest or a local plan does not. */
  locked: boolean;
  /** Why it is disabled, as the tooltip. */
  lockedTitle?: string;
  onChange: (mode: Mode) => void;
}) {
  const options: SegmentOption<Mode>[] = [
    { value: "guided", label: "Guided" },
    ...(describable ? [{ value: "describe" as const, label: "Describe & upload" }] : []),
    ...(locked
      ? [
          {
            value: "describe" as const,
            label: "Describe & upload",
            disabled: true,
            title: lockedTitle,
          },
        ]
      : []),
    { value: "blank", label: "Blank" },
  ];
  return (
    <SegmentedControl
      name="new-scenario-mode"
      ariaLabel="How to start"
      options={options}
      value={mode}
      onChange={onChange}
    />
  );
}

function GuidedSetup(
  props: NewScenarioProps & {
    blank: boolean;
    step: number;
    onStep: (step: number) => void;
    modeSwitch: ReactNode;
  },
) {
  const { blank, onStep: setStep } = props;
  const [profiles, setProfiles] = useState(props.returnProfiles ?? []);
  const [stepError, setStepError] = useState<string>();
  // The Assume step shows its defaults as a summary until asked to change them.
  const [customizing, setCustomizing] = useState(false);
  // A cloud plan's draft keeps its original key; a local plan's is its own,
  // since the profile ids in it belong to that home's library.
  const api = planApiFor(props.home);
  const key = `finplan-setup-v1:${props.defaults.id}${props.home === "local" ? ":local" : ""}`;
  const [draft, setDraft] = useState<SetupDraft>(() => {
    const profileIds = defaultReturnProfileIds(props.returnProfiles ?? []);
    const empty: SetupDraft = {
      request_id: crypto.randomUUID(),
      // Asked for last, once the plan is worth keeping.
      name: freshName(props.takenNames ?? []),
      start_date: new Date().toISOString().slice(0, 10),
      birth_date: props.defaults.birth_date ?? "",
      duration_years: props.defaults.default_duration_years,
      retirement_age: 65,
      cash: 0,
      retirement_401k: 0,
      investments: 0,
      stock_percent: 60,
      cash_profile_id: profileIds.cash,
      stock_profile_id: profileIds.stocks,
      bond_profile_id: profileIds.bonds,
      investment_tax_status: "Taxable",
      annual_income: 0,
      retirement_401k_contribution_percent: 0,
      annual_spending: 0,
      retirement_spending: 0,
      monthly_spending: 0,
      retirement_spending_percent: DEFAULT_RETIREMENT_SPENDING_PERCENT,
      retirement_spending_choice: SUGGESTED_CHOICE,
      inflation_profile_id: props.inflationProfiles[0]?.id ?? null,
      tax_config_id: defaultTaxConfig(props.taxConfigs)?.id ?? null,
      fund_from_investments: true,
      assumptions_confirmed: false,
      has_cash: null,
      has_401k: null,
      has_other_investments: null,
      contribute_401k: false,
    };
    try {
      const value = localStorage.getItem(key);
      if (!value) return empty;
      // A draft saved before retirement spending was a percent kept it in dollars.
      const { monthly_retirement_spending: legacyRetirement, ...saved } = JSON.parse(value) as Partial<SetupDraft> & {
        monthly_retirement_spending?: number;
      };
      const monthlySpending = saved.monthly_spending ?? (saved.annual_spending ?? 0) / 12;
      return {
        ...empty,
        ...saved,
        name: saved.name?.trim() ? saved.name : empty.name,
        retirement_401k: saved.retirement_401k ?? 0,
        retirement_401k_contribution_percent:
          saved.retirement_401k_contribution_percent ?? 0,
        contribute_401k:
          typeof saved.contribute_401k === "boolean"
            ? saved.contribute_401k
            : (saved.retirement_401k_contribution_percent ?? 0) > 0,
        monthly_spending: monthlySpending,
        retirement_spending_percent:
          saved.retirement_spending_percent ??
          (legacyRetirement && monthlySpending > 0
            ? Math.round((legacyRetirement / monthlySpending) * 100)
            : DEFAULT_RETIREMENT_SPENDING_PERCENT),
        has_cash:
          typeof saved.has_cash === "boolean" ? saved.has_cash : (saved.cash ?? 0) > 0 || null,
        has_401k:
          typeof saved.has_401k === "boolean"
            ? saved.has_401k
            : (saved.retirement_401k ?? 0) > 0 || null,
        has_other_investments:
          typeof saved.has_other_investments === "boolean"
            ? saved.has_other_investments
            : (saved.investments ?? 0) > 0 || null,
      };
    } catch {
      return empty;
    }
  });
  const submit = useSubmit();
  // Blank is the details step alone.
  const step = blank ? 0 : props.step;

  useEffect(() => {
    const load = props.returnProfiles
      ? Promise.resolve(props.returnProfiles)
      : api.returnProfiles.list();
    void load
      .then((loaded) => {
        const profileIds = defaultReturnProfileIds(loaded);
        setProfiles(loaded);
        setDraft((current) => ({
          ...current,
          cash_profile_id: current.cash_profile_id || profileIds.cash,
          stock_profile_id: current.stock_profile_id || profileIds.stocks,
          bond_profile_id: current.bond_profile_id || profileIds.bonds,
        }));
      })
      .catch(() => {});
  }, [api, props.returnProfiles]);
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(draft));
    } catch {}
  }, [draft, key]);
  // A new step starts at the top of the page, not where the last one was scrolled to.
  useEffect(() => {
    window.scrollTo({ top: 0 });
  }, [step]);

  const patch = (value: Partial<SetupDraft>) => {
    setStepError(undefined);
    setDraft((current) => ({
      ...current,
      ...value,
      assumptions_confirmed: value.assumptions_confirmed ?? false,
    }));
  };
  const number = (label: string, field: AmountField, max?: number, dollars = false) => (
    <Field label={label}>
      <NumberInput
        aria-label={label}
        value={draft[field]}
        min={0}
        max={max}
        prefix={dollars ? "$" : undefined}
        group={dollars}
        decimals={dollars ? 0 : undefined}
        onValueChange={(value) => patch({ [field]: value })}
      />
    </Field>
  );
  const choice = (
    label: string,
    field: "cash_profile_id" | "stock_profile_id" | "bond_profile_id",
  ) => (
    <Field label={label}>
      <Dropdown
        className="dd-field"
        ariaLabel={label}
        options={profiles.map((profile) => ({ value: profile.id, label: profile.name }))}
        value={draft[field] || null}
        placeholder="Choose an assumption"
        maxMenuHeight={300}
        onChange={(value) => patch({ [field]: value })}
      />
    </Field>
  );

  const cash = draft.has_cash ? draft.cash : 0;
  const retirement401k = draft.has_401k ? draft.retirement_401k : 0;
  const otherInvestments = draft.has_other_investments ? draft.investments : 0;
  const invested = retirement401k + otherInvestments;
  const contributionPercent = draft.contribute_401k
    ? draft.retirement_401k_contribution_percent
    : 0;
  const uncapped401kContribution = draft.annual_income * contributionPercent / 100;
  const annual401kContribution = Math.min(
    uncapped401kContribution,
    EMPLOYEE_401K_DEFERRAL_LIMIT_2026,
  );
  const contributionIsCapped = uncapped401kContribution > annual401kContribution;
  const annualWorkingSpending = draft.monthly_spending * 12;
  const monthlyRetirementSpending =
    (draft.monthly_spending * draft.retirement_spending_percent) / 100;
  const taxConfig = props.taxConfigs.find((tax) => tax.id === draft.tax_config_id);
  // The first working year, as the take-home line shows it. Before the Saving
  // step has been answered there is no deferral yet.
  const firstYear = takeHome(
    draft.annual_income,
    annual401kContribution,
    annualWorkingSpending,
    taxConfig,
  );
  const hasInvestments = invested > 0 || annual401kContribution > 0;
  const wealth = cash + invested;
  const end =
    draft.start_date && draft.duration_years > 0 && draft.duration_years <= 120
      ? addYears(draft.start_date, draft.duration_years)
      : null;

  // Blank names the plan on its one page; Guided at the end.
  const nameStep = blank ? 0 : steps.length - 1;
  const takenName = (props.takenNames ?? []).find(
    (name) => name.trim().toLowerCase() === draft.name.trim().toLowerCase(),
  );
  const stepProblem = (index: number): string | undefined => {
    if (index === nameStep && !draft.name.trim()) return "Name the plan.";
    if (index === nameStep && takenName) return `You already have a plan named ${takenName}.`;
    if (index === 0 && (!draft.birth_date || !end))
      return "Enter a birth date and a valid plan length.";
    if (index === 0 && draft.birth_date > draft.start_date)
      return "Birth date must be on or before the plan start date.";
    if (index === 1 && draft.has_cash == null) return "Choose Yes or No to continue.";
    if (index === 2 && draft.has_401k == null) return "Choose Yes or No to continue.";
    if (index === 2 && draft.has_401k && draft.retirement_401k <= 0)
      return "Enter your current 401(k) balance.";
    if (index === 3 && draft.has_other_investments == null)
      return "Choose Yes or No to continue.";
    if (index === 3 && draft.has_other_investments && draft.investments <= 0)
      return "Enter the current value of your other investments.";
    if (index === 5 && draft.contribute_401k && draft.annual_income <= 0)
      return "Enter a positive annual gross salary before adding a 401(k) contribution.";
    if (
      index === 5 &&
      draft.contribute_401k &&
      draft.retirement_401k_contribution_percent <= 0
    )
      return "Choose a positive 401(k) contribution percentage.";
    if (index === 6 && !draft.cash_profile_id)
      return "Choose a cash return assumption.";
    if (index === 6 && hasInvestments && (!draft.stock_profile_id || !draft.bond_profile_id))
      return "Choose stock and bond return assumptions.";
    return undefined;
  };
  const next = () => {
    const problem = stepProblem(step);
    if (problem) {
      setStepError(problem);
      return;
    }
    setStepError(undefined);
    setStep(step + 1);
  };
  const go = (to: number) => {
    setStepError(undefined);
    setStep(to);
  };
  const createBlank = () => {
    const problem = stepProblem(0);
    if (problem) {
      setStepError(problem);
      return;
    }
    submit.run(async () => {
      const scenario = await api.scenarios.create({
        name: draft.name.trim(),
        start_date: draft.start_date,
        birth_date: draft.birth_date,
        duration_years: draft.duration_years,
        inflation_profile_id: draft.inflation_profile_id,
        tax_config_id: draft.tax_config_id,
      });
      localStorage.removeItem(key);
      props.onCreated(scenario, "plan");
    }, props.onClose);
  };
  const finish = () => {
    // Steps can be skipped from the strip, so every one is checked here, and
    // the first with something missing is opened with the problem on it.
    const unfinished = steps.findIndex((_, index) => stepProblem(index) != null);
    if (unfinished >= 0) {
      setStep(unfinished);
      setStepError(stepProblem(unfinished));
      return;
    }
    if (!draft.assumptions_confirmed) {
      setStepError("Confirm the review before creating your plan.");
      return;
    }
    submit.run(async () => {
      const plan = setupPlanOf(draft);
      const result = await api.scenarios.setup({
        ...plan,
        cash,
        retirement_401k: retirement401k,
        investments: otherInvestments,
        fund_from_investments: hasInvestments && draft.fund_from_investments,
      });
      const scenario = await api.scenarios.get(result.scenario_id);
      localStorage.removeItem(key);
      props.onCreated(scenario, "results");
    }, props.onClose);
  };
  const onSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (blank) createBlank();
    else if (step < steps.length - 1) next();
    else finish();
  };

  // The plan the answers describe, as `/scenarios/setup` will write it.
  const profileName = (id: number) => profiles.find((profile) => profile.id === id)?.name;
  const mix =
    `${draft.stock_percent}% ${profileName(draft.stock_profile_id) ?? "stocks"}, ` +
    `${100 - draft.stock_percent}% ${profileName(draft.bond_profile_id) ?? "bonds"}`;
  const accounts: PreviewAccount[] = [];
  if (draft.has_cash != null || step > 1) {
    accounts.push({
      name: "Checking",
      kind: "Cash",
      tax: { label: "Bank", tone: "neutral" },
      profile: profileName(draft.cash_profile_id) ?? "Cash",
      balance: cash,
    });
  }
  if (retirement401k > 0 || annual401kContribution > 0) {
    accounts.push({
      name: "401(k)",
      kind: "Retirement",
      tax: TAX_TAGS.TaxDeferred,
      profile: mix,
      balance: retirement401k,
    });
  }
  if (otherInvestments > 0) {
    accounts.push({
      name: "Other investments",
      kind: "Investment",
      tax: TAX_TAGS[draft.investment_tax_status] ?? TAX_TAGS.Taxable,
      profile: mix,
      balance: otherInvestments,
    });
  }
  // Guided setup's events follow these parameters (`/scenarios/setup`
  // creates them only when an event does).
  const retireAt = { slot: `Retirement age (${draft.retirement_age})` };
  const funding: PreviewEvent["parts"] =
    hasInvestments && draft.fund_from_investments
      ? ["· shortfall from", { slot: "investments" }]
      : [];
  const events: PreviewEvent[] = [];
  if (draft.annual_income > 0) {
    events.push({
      name: "Salary until retirement",
      parts: [
        "every year until",
        retireAt,
        "· Income of",
        { expr: `inflation(${money(draft.annual_income - annual401kContribution)})` },
        "into",
        { slot: "Checking" },
        ...(annual401kContribution > 0
          ? ["· Contribution of", { expr: `inflation(${money(annual401kContribution)})` }, "into", { slot: "401(k)" }]
          : []),
      ],
      fresh: step === INCOME_STEP || (step === SAVING_STEP && annual401kContribution > 0),
    });
  }
  if (draft.monthly_spending > 0) {
    events.push({
      name: "Spending before retirement",
      parts: [
        "every year until",
        retireAt,
        "· Expense of",
        { expr: `inflation($"Monthly spending" * 12)` },
        "from",
        { slot: "Checking" },
        ...funding,
      ],
      fresh: step === INCOME_STEP || (step === ASSUME_STEP && funding.length > 0),
    });
  }
  const retirementRule = setupRule(
    draft.retirement_spending_choice,
    monthlyRetirementSpending * 12,
    draft.cash + draft.retirement_401k + draft.investments,
  );
  if (monthlyRetirementSpending > 0) {
    events.push({
      name: "Retirement spending",
      parts: retirementRule
        ? [
          "every month from",
          retireAt,
          "· Spending by",
          { expr: describeRule(retirementRule) },
          "from",
          { slot: "Checking" },
          ...funding,
        ]
        : [
          "every year from",
          retireAt,
          "· Expense of",
          { expr: `inflation(${money(monthlyRetirementSpending * 12)})` },
          "from",
          { slot: "Checking" },
          ...funding,
        ],
      fresh: step === INCOME_STEP || (step === ASSUME_STEP && funding.length > 0),
    });
  }
  const parameters: PreviewParameter[] = [];
  if (events.length > 0) {
    parameters.push({
      name: "Retirement age",
      value: `${draft.retirement_age} years`,
      // Every event starts or stops at it.
      usedBy: events.map((event) => event.name).join(", "),
      fresh: step === INCOME_STEP,
    });
  }
  if (draft.monthly_spending > 0) {
    parameters.push({
      name: "Monthly spending",
      value: money(draft.monthly_spending),
      usedBy: "Spending before retirement",
      fresh: step === INCOME_STEP,
    });
  }
  const pending = [
    step < INCOME_STEP &&
      "Step 5 adds your salary and spending as yearly events, with your retirement age and monthly spending as parameters.",
    step < SAVING_STEP && "Step 6 can add 401(k) contributions to Salary.",
    step < ASSUME_STEP &&
      "Step 7 sets return, inflation and tax assumptions, and whether savings cover spending when checking runs short.",
  ].filter((line): line is string => typeof line === "string");
  const assumptions =
    step >= ASSUME_STEP
      ? [
          {
            label: "Inflation",
            value:
              props.inflationProfiles.find((profile) => profile.id === draft.inflation_profile_id)?.name ??
              "No inflation",
          },
          {
            label: "Taxes",
            value: props.taxConfigs.find((tax) => tax.id === draft.tax_config_id)?.name ?? "No taxes modeled",
          },
          {
            label: "Spending funding",
            value: hasInvestments && draft.fund_from_investments ? "Checking, then savings" : "Checking only",
          },
        ]
      : undefined;

  // The Assume step's summary: each default with where it comes from.
  const profileDescription = (id: number) =>
    profiles.find((profile) => profile.id === id)?.description ?? undefined;
  const inflationProfile = props.inflationProfiles.find(
    (profile) => profile.id === draft.inflation_profile_id,
  );
  const assumptionRows: { label: string; value: string; why?: string }[] = [
    ...(hasInvestments
      ? [
          {
            label: "Investment mix",
            value: `${draft.stock_percent}% stocks, ${100 - draft.stock_percent}% bonds`,
          },
          {
            label: "Stock returns",
            value: profileName(draft.stock_profile_id) ?? "",
            why: profileDescription(draft.stock_profile_id),
          },
          {
            label: "Bond returns",
            value: profileName(draft.bond_profile_id) ?? "",
            why: profileDescription(draft.bond_profile_id),
          },
        ]
      : []),
    {
      label: "Cash returns",
      value: profileName(draft.cash_profile_id) ?? "",
      why: profileDescription(draft.cash_profile_id),
    },
    {
      label: "Inflation",
      value: inflationProfile?.name ?? "No inflation",
      why: inflationProfile?.description ?? undefined,
    },
    {
      label: "Taxes",
      value: taxConfig?.name ?? "No taxes modeled",
      why: taxConfig?.description ?? undefined,
    },
    ...(hasInvestments
      ? [
          {
            label: "When checking runs short",
            value: draft.fund_from_investments
              ? "Withdraw from savings, in a tax-aware order"
              : "Checking only; investments are not withdrawn",
          },
        ]
      : []),
  ];

  const age = draft.birth_date && draft.start_date >= draft.birth_date
    ? Math.floor(yearsBetween(draft.birth_date, draft.start_date))
    : null;
  // Not the plan's name: a label that grows as it is typed shifts the bar beside it.
  const title = "New scenario";
  const error = stepError ?? submit.error;

  return (
    <div className="ns-page">
      <SetupBar title={title} modeSwitch={props.modeSwitch} end={<span className="ns-mut">Draft saved on this device</span>}>
        {end && (
          <span>
            {draft.start_date} · {draft.duration_years} yrs
          </span>
        )}
        {draft.birth_date && (
          <>
            <span className="ns-bar-sep">|</span>
            <span>
              born {draft.birth_date}
              {age != null && <span className="ns-mut"> · {age} today</span>}
            </span>
          </>
        )}
      </SetupBar>

      <div className="ns-body ns-body-guided">
        <form className="ns-col" aria-label={blank ? "Blank scenario" : `Set up your plan — ${steps[step].short}`} onSubmit={onSubmit}>
          {!blank && (
            <nav className="ns-steps" aria-label="Steps">
              {steps.map((entry, index) => {
                const state = index < step ? "done" : index === step ? "on" : "todo";
                return (
                  <button
                    key={entry.short}
                    type="button"
                    className={`ns-step ${state}`}
                    disabled={state === "on"}
                    aria-current={state === "on" ? "step" : undefined}
                    title={state === "on" ? undefined : `Go to ${entry.short}`}
                    onClick={() => go(index)}
                  >
                    <div className="stat-l">{index + 1}</div>
                    <div className="ns-step-name">{entry.short}</div>
                  </button>
                );
              })}
            </nav>
          )}

          <div className="ns-scroll">
            <div className="ns-question">
              <div>
                <h2>{blank ? "Start with a blank scenario" : steps[step].title}</h2>
                <p className="ns-mut" style={{ marginTop: 8 }}>
                  {blank
                    ? "Just the plan's name, dates and assumptions. You add accounts and events yourself afterwards."
                    : steps[step].lead}
                </p>
              </div>

              {step === nameStep && (
                <Field label="Plan name">
                  <Input
                    aria-label="Plan name"
                    required
                    value={draft.name}
                    onChange={(event) => patch({ name: event.target.value })}
                  />
                </Field>
              )}

              {step === 0 && (
                <>
                  <DialogRow>
                    <Field label="Birth date">
                      <DateInput
                        required
                        value={draft.birth_date}
                        onValueChange={(birth_date) => patch({ birth_date })}
                      />
                    </Field>
                    {blank ? (
                      <div />
                    ) : (
                      number("Retirement age", "retirement_age", 120)
                    )}
                  </DialogRow>
                  <DialogRow>
                    <Field label="Plan start date">
                      <DateInput
                        required
                        value={draft.start_date}
                        onValueChange={(start_date) => patch({ start_date })}
                      />
                    </Field>
                    {number("Plan length in years", "duration_years", 120)}
                  </DialogRow>
                  <p>
                    {end
                      ? `Plan ends ${end}${
                          draft.birth_date
                            ? `, at age ${Math.floor(yearsBetween(draft.birth_date, end))}`
                            : ""
                        }.`
                      : "Enter a valid plan length."}
                  </p>
                </>
              )}

              {step === 1 && (
                <>
                  <YesNo name="has-cash" value={draft.has_cash} onChange={(has_cash) => patch({ has_cash })} />
                  {draft.has_cash &&
                    number("Total checking and savings balance", "cash", undefined, true)}
                  {draft.has_cash === false && (
                    <p>
                      We&rsquo;ll start checking at $0 so your modeled income and spending still have a
                      cash account.
                    </p>
                  )}
                </>
              )}

              {step === 2 && (
                <>
                  <YesNo name="has-401k" value={draft.has_401k} onChange={(has_401k) => patch({ has_401k })} />
                  {draft.has_401k && number("Current 401(k) balance", "retirement_401k", undefined, true)}
                </>
              )}

              {step === 3 && (
                <>
                  <YesNo
                    name="has-other-investments"
                    value={draft.has_other_investments}
                    onChange={(has_other_investments) => patch({ has_other_investments })}
                  />
                  {draft.has_other_investments && (
                    <>
                      {number("Current value", "investments", undefined, true)}
                      <Field label="What kind of account is this money in?">
                        <Dropdown
                          className="dd-field"
                          ariaLabel="Other investment account type"
                          options={[
                            { value: "Taxable", label: "Taxable brokerage" },
                            { value: "TaxDeferred", label: "Traditional IRA or other tax-deferred" },
                            { value: "TaxFree", label: "Roth IRA or other tax-free" },
                          ]}
                          value={draft.investment_tax_status}
                          onChange={(investment_tax_status) => patch({ investment_tax_status })}
                        />
                      </Field>
                    </>
                  )}
                </>
              )}

              {step === 4 && (
                <>
                  {number("Annual gross salary until retirement", "annual_income", undefined, true)}
                  {number("Monthly spending before retirement", "monthly_spending", undefined, true)}
                  <RangeField
                    label={`Spending in retirement — ${draft.retirement_spending_percent}% of today's${
                      draft.monthly_spending > 0 ? `, ${money(monthlyRetirementSpending)}/month` : ""
                    }`}
                    min={50}
                    max={150}
                    step={5}
                    value={draft.retirement_spending_percent}
                    onValueChange={(retirement_spending_percent) =>
                      patch({ retirement_spending_percent })
                    }
                  />
                  <p className="ns-mut">
                    Many retirees spend 70–100% of what they spent while working, so the plan starts
                    at {DEFAULT_RETIREMENT_SPENDING_PERCENT}%. Salary stops at age{" "}
                    {draft.retirement_age}; retirement spending replaces working-year spending.
                  </p>
                  <Field label="Fixed, or adjust with the market?">
                    <Dropdown
                      className="dd-field"
                      ariaLabel="Retirement spending rule"
                      options={SPENDING_CHOICES.map((choice) => ({
                        value: choice.value,
                        label: choice.value === SUGGESTED_CHOICE ? `${choice.label} (suggested)` : choice.label,
                        detail: choice.detail,
                      }))}
                      value={draft.retirement_spending_choice}
                      onChange={(retirement_spending_choice) => patch({ retirement_spending_choice })}
                    />
                  </Field>
                  <p className="ns-mut">
                    {CHOICE_EXPLAINED[draft.retirement_spending_choice]}
                    {draft.retirement_spending_choice !== "Fixed" &&
                      " Results then show how deep and how often spending is cut, beside whether cash ever runs short."}
                  </p>
                  <TakeHomeLine year={firstYear} taxName={taxConfig?.name} />
                </>
              )}

              {step === 5 && (
                <>
                  <YesNo
                    name="contribute-401k"
                    value={draft.contribute_401k}
                    onChange={(contribute_401k) =>
                      patch({
                        contribute_401k,
                        retirement_401k_contribution_percent:
                          contribute_401k && draft.retirement_401k_contribution_percent === 0
                            ? 6
                            : draft.retirement_401k_contribution_percent,
                      })
                    }
                  />
                  {draft.contribute_401k && (
                    <>
                      <RangeField
                        label={`Contribution — ${draft.retirement_401k_contribution_percent}% of gross salary`}
                        min={0}
                        max={100}
                        step={1}
                        value={draft.retirement_401k_contribution_percent}
                        onValueChange={(retirement_401k_contribution_percent) =>
                          patch({ retirement_401k_contribution_percent })
                        }
                      />
                      <p>
                        <strong>${annual401kContribution.toLocaleString()}</strong> will be contributed
                        each year until retirement and invested at your selected stock/bond mix.
                        {contributionIsCapped && " Your selected percentage exceeds the annual cap."}
                      </p>
                      <p className="ns-mut">
                        Capped at $24,500, the 2026 basic employee-deferral limit. This starter does
                        not add employer matching or age-based catch-up contributions.
                      </p>
                      <TakeHomeLine year={firstYear} taxName={taxConfig?.name} />
                    </>
                  )}
                </>
              )}

              {step === 6 && !customizing && !stepProblem(6) && (
                <>
                  <section className="ns-assume" aria-label="Assumptions">
                    {assumptionRows.map((row) => (
                      <div key={row.label} className="ns-assume-row">
                        <span className="ns-lbl">{row.label}</span>
                        <span>
                          {row.value}
                          {row.why && <span className="ns-mut"> · {row.why}</span>}
                        </span>
                      </div>
                    ))}
                  </section>
                  <Button className="ns-assume-change" onClick={() => setCustomizing(true)}>
                    Change assumptions
                  </Button>
                  <p className="ns-mut">
                    Returns are randomized around these historical averages in every simulation, so
                    the results show good and bad markets, not one guess. Rates are simplified and
                    do not cover every deduction, benefit, or jurisdiction.
                  </p>
                </>
              )}

              {step === 6 && (customizing || stepProblem(6)) && (
                <>
                  {choice("Cash return assumption", "cash_profile_id")}
                  {hasInvestments && (
                    <>
                      {number("Stock allocation percent", "stock_percent", 100)}
                      <DialogRow>
                        {choice("Stock return assumption", "stock_profile_id")}
                        {choice("Bond return assumption", "bond_profile_id")}
                      </DialogRow>
                      <p>
                        Opening investments: ${(invested * draft.stock_percent / 100).toLocaleString()} stocks + ${
                          (invested * (1 - draft.stock_percent / 100)).toLocaleString()
                        } bonds = ${invested.toLocaleString()}. The same starter allocation is applied
                        to each investment account and new 401(k) contributions.
                      </p>
                      <Field label="Spending funding">
                        <Dropdown
                          className="dd-field"
                          ariaLabel="Spending funding"
                          options={[
                            { value: "checking", label: "Checking only — no investment withdrawals" },
                            {
                              value: "investments",
                              label: "Checking, then withdraw from savings",
                            },
                          ]}
                          value={draft.fund_from_investments ? "investments" : "checking"}
                          onChange={(source) =>
                            patch({ fund_from_investments: source === "investments" })
                          }
                        />
                      </Field>
                      <p className="ns-mut">
                        When authorized, annual expenses draw only the checking shortfall from your
                        investment accounts using a tax-aware order. Withdrawals may incur gains tax,
                        income tax, or early-withdrawal penalties.
                      </p>
                    </>
                  )}
                  <Field label="Inflation assumption">
                    <Dropdown
                      className="dd-field"
                      ariaLabel="Inflation assumption"
                      options={[
                        { value: 0, label: "No inflation" },
                        ...props.inflationProfiles.map((profile) => ({
                          value: profile.id,
                          label: profile.name,
                        })),
                      ]}
                      value={draft.inflation_profile_id ?? 0}
                      maxMenuHeight={300}
                      onChange={(inflation_profile_id) =>
                        patch({ inflation_profile_id: inflation_profile_id || null })
                      }
                    />
                  </Field>
                  <Field label="Tax assumption">
                    <Dropdown
                      className="dd-field"
                      ariaLabel="Tax assumption"
                      options={[
                        { value: 0, label: "No taxes modeled" },
                        ...props.taxConfigs.map((tax) => ({ value: tax.id, label: tax.name })),
                      ]}
                      value={draft.tax_config_id ?? 0}
                      maxMenuHeight={300}
                      onChange={(tax_config_id) => patch({ tax_config_id: tax_config_id || null })}
                    />
                  </Field>
                  <p className="ns-mut">
                    {props.taxConfigs.find((tax) => tax.id === draft.tax_config_id)?.description} Tax
                    labels retain their published vintage and filing status. Rates are simplified and
                    do not cover every deduction, benefit, or jurisdiction. Return assumptions are
                    annual nominal rates, not guaranteed forecasts.
                  </p>
                </>
              )}

              {step === 7 && (
                <>
                  <p>
                    {draft.start_date} to {end} · retire at {draft.retirement_age}
                  </p>
                  <p>
                    Checking and savings ${cash.toLocaleString()} + 401(k) ${retirement401k.toLocaleString()}
                    {" + "}other investments ${otherInvestments.toLocaleString()} = ${wealth.toLocaleString()}
                    {" opening wealth."}
                  </p>
                  {hasInvestments && (
                    <p>
                      Investments start at {draft.stock_percent}% stocks and {100 - draft.stock_percent}% bonds.
                    </p>
                  )}
                  <p>
                    Annual gross salary ${draft.annual_income.toLocaleString()}; monthly spending ${draft.monthly_spending.toLocaleString()} before retirement and {money(monthlyRetirementSpending)} after ({draft.retirement_spending_percent}%){
                      draft.retirement_spending_choice === "Fixed"
                        ? ", fixed"
                        : `, adjusting with the market (${
                          SPENDING_CHOICES.find((c) => c.value === draft.retirement_spending_choice)?.label
                        })`
                    }.
                  </p>
                  <TakeHomeLine year={firstYear} taxName={taxConfig?.name} />
                  {annual401kContribution > 0 && (
                    <p>
                      401(k) contribution: {contributionPercent}% of gross salary, ${annual401kContribution.toLocaleString()} per year at the starting salary.
                    </p>
                  )}
                  <p>
                    Funding: {hasInvestments && draft.fund_from_investments
                      ? "Checking, then withdraw from savings for both working and retirement spending"
                      : "Checking only; investments do not automatically fund spending"}.
                  </p>
                  {(!draft.monthly_spending || !monthlyRetirementSpending) && (
                    <p role="alert">Some years have no spending. Confirm that this is intentional.</p>
                  )}
                  <label style={{ display: "inline-flex", alignItems: "flex-start", gap: 8, fontSize: 13.5 }}>
                    <input
                      type="checkbox"
                      style={{ width: 16, height: 16 }}
                      checked={draft.assumptions_confirmed}
                      onChange={(event) => patch({ assumptions_confirmed: event.target.checked })}
                    />
                    I reviewed the dates, balances, allocation, tax vintage, omitted income and costs,
                    and funding rule.
                  </label>
                </>
              )}

              {error && (
                <p role="alert" style={{ fontSize: 12.5, color: "var(--color-accent-700)" }}>
                  {error}
                </p>
              )}
            </div>
          </div>

          <div className="ns-foot">
            <Button variant="ghost" onClick={props.onClose}>
              Cancel
            </Button>
            {!blank && (
              <span role="status" className="ns-mut" style={{ fontSize: 12 }}>
                Step {step + 1} of {steps.length}
              </span>
            )}
            <div className="ns-foot-end">
              {step > 0 && <Button onClick={() => go(step - 1)}>Back</Button>}
              <Button type="submit" variant="primary" disabled={submit.busy}>
                {submit.busy
                  ? "…"
                  : blank
                    ? "Create blank scenario"
                    : step < steps.length - 1
                      ? "Continue"
                      : "Create and run plan"}
              </Button>
            </div>
          </div>
        </form>

        <div className="ns-col">
          <div className="ns-scroll">
            <SetupPreview
              blank={blank}
              accounts={accounts}
              events={events}
              parameters={parameters}
              pending={blank ? [] : pending}
              assumptions={assumptions}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
