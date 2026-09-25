"use client";
import { useState } from "react";
import { DateInput, Dialog, Field, NumberInput, Select } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Account } from "@/lib/api/types";
import { useSubmit } from "@/lib/hooks/useSubmit";
export function FundingRuleDialog({ scenarioId, accounts, onClose, onSaved }: {
    scenarioId: number;
    accounts: Account[];
    onClose: () => void;
    onSaved: () => void;
}) {
    const [source, setSource] = useState(0);
    const [target, setTarget] = useState(0);
    const [amount, setAmount] = useState(0);
    const [start, setStart] = useState("");
    const submit = useSubmit();
    return <Dialog title="Draft a spending funding rule" onClose={onClose} submitLabel="Save funding rule" busy={submit.busy} error={submit.error} onSubmit={() => submit.run(async () => { if (!source || !target || source === target || amount <= 0 || !start)
        throw new Error("Choose separate source and destination accounts, a positive cash target and a start date."); await api.events.create(scenarioId, { name: `Funding ${accounts.find(a => a.id === target)?.name} from ${accounts.find(a => a.id === source)?.name}`, description: "Explicit monthly cash top-up; user reviewed withdrawal source and modeled tax implications.", enabled: true, fires_once: false, sort_order: -1, trigger: { kind: "Repeating", interval: "Monthly", start_condition: { kind: "Date", on_date: start }, end_condition: null, max_occurrences: null }, effects: [{ kind: "Sweep", to_account_id: target, sources: { mode: "SingleAccount", account_id: source }, amount: { kind: "Expression", source: `max(0, inflation(${amount}) - cash(target))` }, amount_mode: "Net", lot_method: "Fifo", income_type: "TaxFree" }] }); onSaved(); }, onClose)}><Field label="Withdraw from"><Select value={source} onChange={e => setSource(Number(e.target.value))}><option value={0}>Choose an investment account</option>{accounts.filter(a => a.flavor === "Investment").map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</Select></Field><Field label="Fund this account"><Select value={target} onChange={e => setTarget(Number(e.target.value))}><option value={0}>Choose destination</option>{accounts.filter(a => a.flavor === "Bank" || a.flavor === "Investment").map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</Select></Field><Field label="Monthly cash target, plan-start dollars"><NumberInput min={0} value={amount} onValueChange={setAmount}/></Field><Field label="Begin monthly funding"><DateInput value={start} onValueChange={setStart}/></Field><p>Preview: each month starting {start || "on the chosen date"}, top up destination cash to ${amount.toLocaleString()}, adjusted for inflation, using the selected source account only. No withdrawal occurs if cash already meets the target. Oldest lots are sold first. The rule is placed before other events; verify its dates against your expenses.</p><p>Net withdrawals include the engine’s modeled gains, income tax and early withdrawal penalties. A cash target does not guarantee every expense or market path is funded. Save explicitly to create an ordinary editable event.</p></Dialog>;
}
