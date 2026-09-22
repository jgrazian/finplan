"use client";

import { useEffect, useState, type ReactNode } from "react";
import {
  Button,
  DateInput,
  Dialog,
  DialogRow,
  Dropdown,
  Field,
  Input,
  NumberInput,
  RangeField,
  SegmentedControl,
} from "@/components/ui";
import { api } from "@/lib/api/client";
import { http } from "@/lib/api/http";
import type { SetupPlan } from "@/lib/api/generated/SetupPlan";
import type { Profile, Scenario, TaxConfig, UserResponse } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { addYears, yearsBetween } from "@/lib/view/format";

const steps = [
  "Plan details",
  "Cash",
  "401(k)",
  "Other investments",
  "Income and retirement",
  "401(k) contributions",
  "Assumptions",
  "Review",
] as const;

const EMPLOYEE_401K_DEFERRAL_LIMIT_2026 = 24_500;

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
  monthly_retirement_spending: number;
};

type AmountField =
  | "duration_years"
  | "retirement_age"
  | "cash"
  | "retirement_401k"
  | "investments"
  | "stock_percent"
  | "annual_income"
  | "retirement_401k_contribution_percent"
  | "monthly_spending"
  | "monthly_retirement_spending";

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

function Question({ children }: { children: ReactNode }) {
  return <h3 style={{ margin: "2px 0 0", fontSize: 18 }}>{children}</h3>;
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
  delete answers.monthly_retirement_spending;
  plan.retirement_401k_contribution_percent = draft.contribute_401k
    ? draft.retirement_401k_contribution_percent
    : 0;
  plan.annual_spending = draft.monthly_spending * 12;
  plan.retirement_spending = draft.monthly_retirement_spending * 12;
  return plan;
}

export function NewScenarioDialog(props: {
  defaults: UserResponse;
  inflationProfiles: Profile[];
  taxConfigs: TaxConfig[];
  returnProfiles?: Profile[];
  onClose: () => void;
  onCreated: (s: Scenario) => void;
}) {
  const [profiles, setProfiles] = useState(props.returnProfiles ?? []);
  const [step, setStep] = useState(0);
  const [stepError, setStepError] = useState<string>();
  const key = `finplan-setup-v1:${props.defaults.id}`;
  const [draft, setDraft] = useState<SetupDraft>(() => {
    const profileIds = defaultReturnProfileIds(props.returnProfiles ?? []);
    const empty: SetupDraft = {
      request_id: crypto.randomUUID(),
      name: "",
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
      monthly_retirement_spending: 0,
      inflation_profile_id: props.inflationProfiles[0]?.id ?? null,
      tax_config_id: props.taxConfigs[0]?.id ?? null,
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
      const saved = JSON.parse(value) as Partial<SetupDraft>;
      return {
        ...empty,
        ...saved,
        retirement_401k: saved.retirement_401k ?? 0,
        retirement_401k_contribution_percent:
          saved.retirement_401k_contribution_percent ?? 0,
        contribute_401k:
          typeof saved.contribute_401k === "boolean"
            ? saved.contribute_401k
            : (saved.retirement_401k_contribution_percent ?? 0) > 0,
        monthly_spending: saved.monthly_spending ?? (saved.annual_spending ?? 0) / 12,
        monthly_retirement_spending:
          saved.monthly_retirement_spending ?? (saved.retirement_spending ?? 0) / 12,
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
  }, [props.returnProfiles]);
  useEffect(() => {
    try {
      localStorage.setItem(key, JSON.stringify(draft));
    } catch {}
  }, [draft, key]);

  const patch = (value: Partial<SetupDraft>) => {
    setStepError(undefined);
    setDraft((current) => ({
      ...current,
      ...value,
      assumptions_confirmed: value.assumptions_confirmed ?? false,
    }));
  };
  const number = (label: string, field: AmountField, max?: number, money = false) => (
    <Field label={label}>
      <NumberInput
        aria-label={label}
        value={draft[field]}
        min={0}
        max={max}
        prefix={money ? "$" : undefined}
        group={money}
        decimals={money ? 0 : undefined}
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
  const hasInvestments = invested > 0 || annual401kContribution > 0;
  const wealth = cash + invested;
  const end =
    draft.start_date && draft.duration_years > 0 && draft.duration_years <= 120
      ? addYears(draft.start_date, draft.duration_years)
      : null;

  const stepProblem = (index: number): string | undefined => {
    if (index === 0 && (!draft.name.trim() || !draft.birth_date || !end))
      return "Enter a name, birth date, and valid plan length.";
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
    setStep((current) => current + 1);
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
      props.onCreated(scenario);
    }, props.onClose);
  };
  const finish = () => {
    const problem = stepProblem(0) ?? stepProblem(5) ?? stepProblem(6);
    if (problem) {
      setStepError(problem);
      return;
    }
    if (!draft.assumptions_confirmed) {
      setStepError("Confirm the review before creating your plan.");
      return;
    }
    submit.run(async () => {
      const plan = setupPlanOf(draft);
      const result = await http.post<{ scenario_id: number }>("/scenarios/setup", {
        ...plan,
        cash,
        retirement_401k: retirement401k,
        investments: otherInvestments,
        fund_from_investments: hasInvestments && draft.fund_from_investments,
      });
      const scenario = await api.scenarios.get(result.scenario_id);
      localStorage.removeItem(key);
      props.onCreated(scenario);
    }, props.onClose);
  };

  const footer = (
    <div
      style={{ display: "flex", alignItems: "center", flexWrap: "wrap", gap: 8, marginTop: 4 }}
    >
      <Button type="button" onClick={props.onClose}>
        Cancel
      </Button>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginLeft: "auto" }}>
        {step === 0 && (
          <Button type="button" disabled={submit.busy} onClick={createBlank}>
            Create blank scenario
          </Button>
        )}
        {step > 0 && (
          <Button
            type="button"
            onClick={() => {
              setStepError(undefined);
              setStep((current) => current - 1);
            }}
          >
            Back
          </Button>
        )}
        <Button type="submit" variant="primary" disabled={submit.busy}>
          {submit.busy
            ? "…"
            : step === 0
              ? "Continue setup"
              : step < steps.length - 1
                ? "Continue"
                : "Create plan"}
        </Button>
      </div>
    </div>
  );

  return (
    <Dialog
      title={`Set up your plan — ${steps[step]}`}
      onClose={props.onClose}
      onSubmit={() => (step < steps.length - 1 ? next() : finish())}
      busy={submit.busy}
      error={stepError ?? submit.error}
      footer={footer}
    >
      <p role="status" style={{ margin: 0, fontSize: 12 }}>
        Step {step + 1} of {steps.length} · Draft saved on this device
      </p>

      {step === 0 && (
        <>
          <Question>Start with the plan details</Question>
          <Field label="Plan name">
            <Input
              aria-label="Plan name"
              required
              value={draft.name}
              onChange={(event) => patch({ name: event.target.value })}
            />
          </Field>
          <DialogRow>
            <Field label="Birth date">
              <DateInput
                required
                value={draft.birth_date}
                onValueChange={(birth_date) => patch({ birth_date })}
              />
            </Field>
            {number("Retirement age", "retirement_age", 120)}
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
          <p style={{ margin: 0 }}>
            {end
              ? `Plan ends ${end}${
                  draft.birth_date
                    ? `, at age ${Math.floor(yearsBetween(draft.birth_date, end))}`
                    : ""
                }.`
              : "Enter a valid plan length."}{" "}
            Continue through guided setup, or create a blank scenario with these details.
          </p>
        </>
      )}

      {step === 1 && (
        <>
          <Question>Do you have money in a checking or savings account?</Question>
          <YesNo name="has-cash" value={draft.has_cash} onChange={(has_cash) => patch({ has_cash })} />
          {draft.has_cash &&
            number("Total checking and savings balance", "cash", undefined, true)}
          {draft.has_cash === false && (
            <p style={{ margin: 0 }}>
              We&rsquo;ll start checking at $0 so your modeled income and spending still have a
              cash account.
            </p>
          )}
        </>
      )}

      {step === 2 && (
        <>
          <Question>Do you have a 401(k)?</Question>
          <YesNo name="has-401k" value={draft.has_401k} onChange={(has_401k) => patch({ has_401k })} />
          {draft.has_401k && number("Current 401(k) balance", "retirement_401k", undefined, true)}
          <p style={{ margin: 0 }}>
            A 401(k) is modeled as a tax-deferred retirement account. You can refine its
            holdings and contribution rules after setup.
          </p>
        </>
      )}

      {step === 3 && (
        <>
          <Question>Do you have investments outside a 401(k)?</Question>
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
          <Question>How much do you earn and spend?</Question>
          <DialogRow>
            {number("Annual gross salary until retirement", "annual_income", undefined, true)}
            {number("Monthly spending before retirement", "monthly_spending", undefined, true)}
          </DialogRow>
          {number(
            "Monthly spending in retirement",
            "monthly_retirement_spending",
            undefined,
            true,
          )}
          <p style={{ margin: 0 }}>
            Spending is entered per month in plan-start dollars and modeled as an annual
            inflation-adjusted expense. Salary stops at age {draft.retirement_age}; retirement
            spending replaces working-year spending.
          </p>
        </>
      )}

      {step === 5 && (
        <>
          <Question>Would you like to contribute part of your salary to a 401(k)?</Question>
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
              <p style={{ margin: 0 }}>
                <strong>${annual401kContribution.toLocaleString()}</strong> will be contributed
                each year until retirement and invested at your selected stock/bond mix.
                {contributionIsCapped && " Your selected percentage exceeds the annual cap."}
              </p>
              <p style={{ margin: 0 }}>
                Capped at $24,500, the 2026 basic employee-deferral limit. This starter does
                not add employer matching or age-based catch-up contributions.
              </p>
            </>
          )}
        </>
      )}

      {step === 6 && (
        <>
          <Question>Which assumptions should this plan use?</Question>
          {choice("Cash return assumption", "cash_profile_id")}
          {hasInvestments && (
            <>
              {number("Stock allocation percent", "stock_percent", 100)}
              <DialogRow>
                {choice("Stock return assumption", "stock_profile_id")}
                {choice("Bond return assumption", "bond_profile_id")}
              </DialogRow>
              <p style={{ margin: 0 }}>
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
              <p style={{ margin: 0 }}>
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
          <p style={{ margin: 0 }}>
            {props.taxConfigs.find((tax) => tax.id === draft.tax_config_id)?.description} Tax
            labels retain their published vintage and filing status. Rates are simplified and
            do not cover every deduction, benefit, or jurisdiction. Return assumptions are
            annual nominal rates, not guaranteed forecasts.
          </p>
        </>
      )}

      {step === 7 && (
        <>
          <Question>Review your starter plan</Question>
          <p style={{ margin: 0 }}>
            <strong>{draft.name}</strong> · {draft.start_date} to {end} · retire at {draft.retirement_age}
          </p>
          <p style={{ margin: 0 }}>
            Checking and savings ${cash.toLocaleString()} + 401(k) ${retirement401k.toLocaleString()}
            {" + "}other investments ${otherInvestments.toLocaleString()} = ${wealth.toLocaleString()}
            {" opening wealth."}
          </p>
          {hasInvestments && (
            <p style={{ margin: 0 }}>
              Investments start at {draft.stock_percent}% stocks and {100 - draft.stock_percent}% bonds.
            </p>
          )}
          <p style={{ margin: 0 }}>
            Annual gross salary ${draft.annual_income.toLocaleString()}; monthly spending ${draft.monthly_spending.toLocaleString()} before retirement and ${draft.monthly_retirement_spending.toLocaleString()} after.
          </p>
          {annual401kContribution > 0 && (
            <p style={{ margin: 0 }}>
              401(k) contribution: {contributionPercent}% of gross salary, ${annual401kContribution.toLocaleString()} per year at the starting salary.
            </p>
          )}
          <p style={{ margin: 0 }}>
            Funding: {hasInvestments && draft.fund_from_investments
              ? "Checking, then withdraw from savings for both working and retirement spending"
              : "Checking only; investments do not automatically fund spending"}.
          </p>
          {(!draft.monthly_spending || !draft.monthly_retirement_spending) && (
            <p role="alert" style={{ margin: 0 }}>
              Some years have no spending. Confirm that this is intentional.
            </p>
          )}
          <label style={{ display: "inline-flex", alignItems: "flex-start", gap: 8 }}>
            <input
              type="checkbox"
              style={{ width: 16, height: 16 }}
              checked={draft.assumptions_confirmed}
              onChange={(event) => patch({ assumptions_confirmed: event.target.checked })}
            />
            I reviewed the dates, balances, allocation, tax vintage, omitted income and costs,
            and funding rule.
          </label>
          <p style={{ margin: 0 }}>
            Create saves ordinary editable accounts, holdings, and events together. You can
            refine each one in the plan editor.
          </p>
        </>
      )}
    </Dialog>
  );
}
