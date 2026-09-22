"use client";
import { useEffect, useState } from "react";
import { Button, Field } from "@/components/ui";
import { http } from "@/lib/api/http";
import type { Entitlements } from "@/lib/api/generated/Entitlements";
import type { Scenario } from "@/lib/api/types";
import { accessPresentation } from "@/lib/view/access";

export function BillingPanel({ scenarios, readOnly }: { scenarios: Scenario[]; readOnly?: boolean }) {
  const [access, setAccess] = useState<Entitlements | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    let active = true;
    http.get<Entitlements>("/billing/entitlements").then((value) => { if (active) setAccess(value); }).catch((error) => { if (active) setError(error instanceof Error ? error.message : "Could not load plan access."); });
    return () => { active = false; };
  }, []);
  const presentation = access ? accessPresentation(access) : null;
  return <div style={{ maxWidth: 600 }}>
    {error && <p role="alert">{error}</p>}
    {!access ? <p>Loading plan access…</p> : <>
      <h4>{presentation?.label}</h4>
      <p>{presentation?.description}</p>
      {access.hosted && <p>Up to {access.max_iterations.toLocaleString()} iterations per run. Compute limits apply.</p>}
      {presentation?.showBilling && <>
        {!access.pro && <p>Goal seeks used this month: {access.goal_seeks_used_this_month} of {access.goal_seeks_per_month}. Resets at the start of each calendar month (UTC). Accepted jobs count even if canceled or failed.</p>}
        {!access.pro && scenarios.length > 0 && <Field label="Editable plan">
          <select aria-label="Editable plan" disabled={busy || readOnly} value={access.editable_scenario_id ?? ""} onChange={async (event) => {
            setBusy(true); setError("");
            try { setAccess(await http.post<Entitlements>("/billing/editable-plan", { scenario_id: Number(event.target.value) })); }
            catch (error) { setError(error instanceof Error ? error.message : "Could not change the editable plan."); }
            finally { setBusy(false); }
          }}>
            {scenarios.map((scenario) => <option key={scenario.id} value={scenario.id}>{scenario.name}</option>)}
          </select>
        </Field>}
        <p>Existing plans stay readable and exportable after a downgrade. Choosing another editable plan preserves the others.</p>
        <h4>Pro pricing</h4>
        <p>${access.annual_price_usd}/year or ${access.monthly_price_usd}/month.</p>
        <Button disabled>Checkout is not available yet</Button>
        <p>Payments and subscription management will be available after the payment provider is connected.</p>
      </>}
    </>}
  </div>;
}
