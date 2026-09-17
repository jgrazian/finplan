"use client";
import { useEffect, useState } from "react";
import { Button, Field, Input, NumberInput, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Profile, TaxConfig } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
export function AssumptionsEditor({ scenarioId, onChanged, offline }: {
    scenarioId: number;
    onChanged: () => void;
    offline?: boolean;
}) {
    const [taxes, setTaxes] = useState<TaxConfig[]>([]);
    const [inflation, setInflation] = useState<Profile[]>([]);
    const [tax, setTax] = useState(0);
    const [inf, setInf] = useState(0);
    const [custom, setCustom] = useState(false);
    const [name, setName] = useState("");
    const [stateRate, setStateRate] = useState(0);
    const [gains, setGains] = useState(15);
    const [penalty, setPenalty] = useState(10);
    const submit = useSubmit();
    const selected = taxes.find(t => t.id === tax);
    useEffect(() => { let active = true; void Promise.all([api.scenarios.get(scenarioId), api.taxConfigs.list(), api.inflationProfiles.list()]).then(([s, t, i]) => { if (active) {
        setTaxes(t);
        setInflation(i);
        setTax(s.tax_config_id ?? 0);
        setInf(s.inflation_profile_id ?? 0);
    } }).catch(() => { }); return () => { active = false; }; }, [scenarioId]);
    return <details style={{ padding: "12px 24px", borderBottom: "1px solid var(--color-divider)" }}><summary>Review or edit tax and inflation assumptions</summary><div style={{ maxWidth: 600, display: "grid", gap: 12, paddingTop: 12 }}><Field label="Tax profile"><Select value={tax} onChange={e => { setTax(Number(e.target.value)); setCustom(false); }}>{taxes.map(t => <option key={t.id} value={t.id}>{t.name}</option>)}</Select></Field><p>{selected?.description} The label identifies the profile’s vintage and filing assumptions; it is not automatically updated. Tax modeling omits some deductions, credits and jurisdiction-specific rules.</p><Field label="Inflation profile"><Select value={inf} onChange={e => setInf(Number(e.target.value))}>{inflation.map(i => <option key={i.id} value={i.id}>{i.name}</option>)}</Select></Field><p>Returns and inflation are annual nominal assumptions. A fixed asset price can lose purchasing power as prices rise.</p>{selected && !custom && <Button onClick={() => { setCustom(true); setName(`${selected.name} — revised assumptions`); setStateRate(selected.state_rate * 100); setGains(selected.capital_gains_rate * 100); setPenalty(selected.early_withdrawal_penalty_rate * 100); }}>Create a tax assumption copy</Button>}{custom && <><Field label="New tax profile name"><Input value={name} onChange={e => setName(e.target.value)}/></Field><Field label="State tax percent"><NumberInput value={stateRate} min={0} max={100} onValueChange={setStateRate}/></Field><Field label="Capital gains tax percent"><NumberInput value={gains} min={0} max={100} onValueChange={setGains}/></Field><Field label="Early withdrawal penalty percent"><NumberInput value={penalty} min={0} max={100} onValueChange={setPenalty}/></Field><p>Federal brackets remain those of {selected?.name}. Saving creates a separate profile so other plans keep their assumptions.</p></>}<Button disabled={offline || submit.busy} onClick={() => submit.run(async () => { let id = tax; if (custom && selected) {
        if (!name.trim())
            throw new Error("Enter a name for the new assumption profile.");
        const copied = await api.taxConfigs.create({ name: name.trim(), description: `User revision of ${selected.name}. Federal brackets preserved from source.`, state_rate: stateRate / 100, capital_gains_rate: gains / 100, early_withdrawal_penalty_rate: penalty / 100, federal_brackets: selected.federal_brackets });
        id = copied.id;
        setTaxes(t => [...t, copied]);
        setTax(id);
        setCustom(false);
    } await api.scenarios.update(scenarioId, { tax_config_id: id || null, inflation_profile_id: inf || null }); }, onChanged)}>Save assumptions</Button>{submit.error && <p role="alert">{submit.error}</p>}</div></details>;
}
