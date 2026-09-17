"use client";
import { useEffect, useState } from "react";
import { Button, DateInput, Dialog, Field, Input, NumberInput, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import { http } from "@/lib/api/http";
import type { Profile, Scenario, TaxConfig, UserResponse } from "@/lib/api/types";
import type { SetupPlan } from "@/lib/api/generated/SetupPlan";
import { useSubmit } from "@/lib/hooks/useSubmit";
import { addYears, yearsBetween } from "@/lib/view/format";
import { AdvancedScenarioDialog } from "./AdvancedScenarioDialog";
const steps = ["Household and dates", "Accounts and allocation", "Income and retirement", "Funding and assumptions", "Review"];
export function NewScenarioDialog(props: {
    defaults: UserResponse;
    inflationProfiles: Profile[];
    taxConfigs: TaxConfig[];
    returnProfiles?: Profile[];
    onClose: () => void;
    onCreated: (s: Scenario) => void;
}) {
    const [advanced, setAdvanced] = useState(false);
    const [profiles, setProfiles] = useState(props.returnProfiles ?? []);
    const [step, setStep] = useState(0);
    const key = `finplan-setup-v1:${props.defaults.id}`;
    const [draft, setDraft] = useState<SetupPlan>(() => {
        try {
            const saved = localStorage.getItem(key);
            if (saved)
                return JSON.parse(saved);
        }
        catch { }
        return { request_id: crypto.randomUUID(), name: "", start_date: new Date().toISOString().slice(0, 10), birth_date: props.defaults.birth_date ?? "", duration_years: props.defaults.default_duration_years, retirement_age: 65, cash: 0, investments: 0, stock_percent: 60, cash_profile_id: 0, stock_profile_id: 0, bond_profile_id: 0, investment_tax_status: "Taxable", annual_income: 0, annual_spending: 0, retirement_spending: 0, inflation_profile_id: props.inflationProfiles[0]?.id ?? null, tax_config_id: props.taxConfigs[0]?.id ?? null, fund_from_investments: false, assumptions_confirmed: false };
    });
    const submit = useSubmit();
    useEffect(() => { if (!props.returnProfiles)
        void api.returnProfiles.list().then(setProfiles).catch(() => { }); }, [props.returnProfiles]);
    useEffect(() => { try {
        localStorage.setItem(key, JSON.stringify(draft));
    }
    catch { } }, [draft, key]);
    const patch = (v: Partial<SetupPlan>) => setDraft(d => ({ ...d, ...v, assumptions_confirmed: v.assumptions_confirmed ?? false }));
    const number = (label: string, field: "duration_years" | "retirement_age" | "cash" | "investments" | "stock_percent" | "annual_income" | "annual_spending" | "retirement_spending", max?: number) => <Field label={label}><NumberInput aria-label={label} value={draft[field]} min={0} max={max} onValueChange={v => patch({ [field]: v })}/></Field>;
    const choice = (label: string, field: "cash_profile_id" | "stock_profile_id" | "bond_profile_id", items = profiles) => <Field label={label}><Select aria-label={label} value={draft[field]} required onChange={e => patch({ [field]: Number(e.target.value) })}><option value={0}>Choose an assumption</option>{items.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></Field>;
    if (advanced)
        return <AdvancedScenarioDialog {...props}/>;
    const end = draft.start_date && draft.duration_years > 0 && draft.duration_years <= 120 ? addYears(draft.start_date, draft.duration_years) : null;
    const finish = () => submit.run(async () => {
        if (!draft.name.trim() || !draft.birth_date || !end)
            throw new Error("Enter a name, birth date and valid plan length.");
        if (!draft.cash_profile_id || !draft.stock_profile_id || !draft.bond_profile_id)
            throw new Error("Choose cash, stock and bond return assumptions.");
        if (!draft.assumptions_confirmed)
            throw new Error("Confirm the review before creating your plan.");
        const result = await http.post<{
            scenario_id: number;
        }>("/scenarios/setup", draft);
        const scenario = await api.scenarios.get(result.scenario_id);
        localStorage.removeItem(key);
        props.onCreated(scenario);
    }, props.onClose);
    return <Dialog title={`Set up your plan — ${steps[step]}`} onClose={props.onClose} onSubmit={() => step < 4 ? setStep(step + 1) : finish()} submitLabel={step < 4 ? "Continue" : "Create reviewed plan"} busy={submit.busy} error={submit.error}>
  <p role="status">Step {step + 1} of 5 · Draft saved on this device</p>
  <div style={{ display: "flex", gap: 8 }}>{step > 0 && <Button onClick={() => setStep(step - 1)}>Back</Button>}<Button onClick={() => setAdvanced(true)}>Use advanced editor</Button></div>
  {step === 0 && <><Field label="Plan name"><Input aria-label="Plan name" required value={draft.name} onChange={e => patch({ name: e.target.value })}/></Field><Field label="Birth date"><DateInput required value={draft.birth_date} onValueChange={v => patch({ birth_date: v })}/></Field><Field label="Start date"><DateInput required value={draft.start_date} onValueChange={v => patch({ start_date: v })}/></Field>{number("Plan length in years", "duration_years", 120)}<p>{end ? `Plan ends ${end}${draft.birth_date ? `, at age ${Math.floor(yearsBetween(draft.birth_date, end))}` : ""}. Choose a horizon covering your lifetime needs.` : "Enter a valid plan length."}</p><p>This starter models one person’s retirement dates. Add a partner’s income, benefits and other household changes in the plan editor.</p><Button onClick={() => patch({ name: "Fictional demo — retirement", birth_date: "1981-01-01", cash: 50000, investments: 500000, annual_income: 100000, annual_spending: 40000, retirement_spending: 40000, duration_years: 50 })}>Fill fictional demo amounts</Button></>}
  {step === 1 && <>{number("Checking balance", "cash")}{number("Total investment value", "investments")}<Field label="Investment account type"><Select value={draft.investment_tax_status} onChange={e => patch({ investment_tax_status: e.target.value })}><option value="Taxable">Taxable brokerage</option><option value="TaxDeferred">Tax-deferred retirement</option><option value="TaxFree">Tax-free retirement</option></Select></Field>{number("Stock allocation percent", "stock_percent", 100)}{choice("Cash return assumption", "cash_profile_id")}{choice("Stock return assumption", "stock_profile_id")}{choice("Bond return assumption", "bond_profile_id")}<p>Invested: ${(draft.investments * draft.stock_percent / 100).toLocaleString()} stocks + ${(draft.investments * (1 - draft.stock_percent / 100)).toLocaleString()} bonds = ${draft.investments.toLocaleString()}. Investment cash is $0. Opening cost basis equals opening value; edit existing gains/losses after setup.</p></>}
  {step === 2 && <>{number("Annual gross salary until retirement", "annual_income")}{number("Annual spending before retirement", "annual_spending")}{number("Retirement age", "retirement_age", 120)}{number("Annual spending in retirement", "retirement_spending")}<p>Amounts are in plan-start dollars, adjusted by your inflation profile. Salary stops at retirement; retirement spending replaces working-year spending. Events repeat yearly. Add benefits, contributions and other income in the advanced plan editor.</p></>}
  {step === 3 && <><Field label="Spending funding"><Select aria-label="Spending funding" value={draft.fund_from_investments ? "investments" : "checking"} onChange={e => patch({ fund_from_investments: e.target.value === "investments" })}><option value="checking">Checking only — no investment withdrawals</option><option value="investments">Checking, then withdraw from Investments</option></Select></Field><p>When authorized, before each annual expense a funding rule withdraws only the checking shortfall from Investments, using oldest lots first and net-of-modeled-tax amounts. Withdrawals may incur gains tax, income tax or early withdrawal penalties. This rule does not guarantee funding in every market path.</p><Field label="Inflation assumption"><Select value={draft.inflation_profile_id ?? 0} onChange={e => patch({ inflation_profile_id: Number(e.target.value) || null })}><option value={0}>No inflation</option>{props.inflationProfiles.map(p => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></Field><Field label="Tax assumption"><Select value={draft.tax_config_id ?? 0} onChange={e => patch({ tax_config_id: Number(e.target.value) || null })}><option value={0}>No taxes modeled</option>{props.taxConfigs.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}</Select></Field><p>{props.taxConfigs.find(t => t.id === draft.tax_config_id)?.description} Tax labels retain their published vintage and filing status. Rates are simplified and do not cover every deduction, benefit or jurisdiction. Return assumptions are annual nominal rates, not guaranteed forecasts.</p></>}
  {step === 4 && <><p><strong>{draft.name}</strong> · {draft.start_date} to {end} · retire at {draft.retirement_age}</p><p>Checking ${draft.cash.toLocaleString()} + investments ${draft.investments.toLocaleString()} = ${(draft.cash + draft.investments).toLocaleString()} opening wealth. {draft.stock_percent}% stocks; {100 - draft.stock_percent}% bonds.</p><p>Annual gross salary ${draft.annual_income.toLocaleString()}; spending ${draft.annual_spending.toLocaleString()} before retirement and ${draft.retirement_spending.toLocaleString()} after.</p><p>Funding: {draft.fund_from_investments ? "Checking, then Investments via explicit annual shortfall withdrawals" : "Checking only; investments do not automatically fund spending"}.</p>{(!draft.annual_spending || !draft.retirement_spending) && <p role="alert">Some years have no spending. Confirm intentional omissions.</p>}<label><input type="checkbox" checked={draft.assumptions_confirmed} onChange={e => patch({ assumptions_confirmed: e.target.checked })}/> I reviewed the dates, allocation, tax vintage, omitted income/costs and explicit funding rule.</label><p>Create saves ordinary editable accounts, holdings and events together. Review preflight before your first simulation.</p></>}
 </Dialog>;
}
