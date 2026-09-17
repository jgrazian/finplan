"use client";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui";
import { http } from "@/lib/api/http";
import type { PreflightReport } from "@/lib/api/generated/PreflightReport";
export function PreflightPanel({ scenarioId, onReviewPlan, onRun }: {
    scenarioId: number;
    onReviewPlan: (section?: string, recordId?: number) => void;
    onRun?: () => void;
}) {
    const [report, setReport] = useState<PreflightReport | null>(null);
    const [error, setError] = useState("");
    const [accepted, setAccepted] = useState(false);
    useEffect(() => { let active = true; void http.get<PreflightReport>(`/scenarios/${scenarioId}/preflight`).then(r => { if (active)
        setReport(r); }).catch(e => { if (active)
        setError(String(e)); }); return () => { active = false; }; }, [scenarioId]);
    return <section aria-label="Plan preflight" style={{ padding: 20, borderBottom: "1px solid var(--color-divider)" }}><h3>Review before running</h3>{error && <p role="alert">Unable to review this plan: {error}</p>}{!report && !error && <p>Checking plan inputs…</p>}{report && <><p>These checks identify missing inputs; they cannot prove the plan will remain funded.</p>{report.issues.map((issue, i) => <div key={`${issue.code}:${i}`} style={{ marginBottom: 12 }}><strong>{issue.severity === "error" ? "Must fix" : "Review"}: </strong>{issue.message} <Button onClick={() => onReviewPlan(issue.section, issue.record_id ?? undefined)}>Review {issue.section}</Button></div>)}{report.can_run && report.issues.length > 0 && <label><input type="checkbox" checked={accepted} onChange={e => setAccepted(e.target.checked)}/> I reviewed these assumptions and intentional omissions.</label>}{onRun && <div style={{ marginTop: 12 }}><Button variant="primary" disabled={!report.can_run || (report.issues.length > 0 && !accepted)} onClick={onRun}>Run reviewed plan</Button></div>}</>}</section>;
}
