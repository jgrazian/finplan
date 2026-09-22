"use client";
import { useEffect, useState } from "react";

import { api } from "@/lib/api/client";
import { historyApi } from "@/lib/api/history";
import type { Results, Run } from "@/lib/api/types";
import type { RunInputs } from "@/lib/api/generated/RunInputs";
import type { Entitlements } from "@/lib/api/generated/Entitlements";
import { comparisonWarnings, changedInputSections } from "@/lib/view/history";
import { reportHtml } from "@/lib/view/report";
import { Button } from "@/components/ui";
const money=(v:number)=>v.toLocaleString("en-US",{style:"currency",currency:"USD",maximumFractionDigits:0});
const rate=(v:number|null)=>v == null ? "Not measured — rerun" : `${(v*100).toFixed(1)}%`;
export function RunHistory({history,selectedRunId,onSelectRun,inputs}:{history:Run[];selectedRunId?:number;onSelectRun:(id:number)=>void;inputs?:RunInputs}) {
  const [access,setAccess]=useState<Entitlements>();
  const pro=access?.pro ?? false;
  const [plans,setPlans]=useState<{id:number;name:string}[]>([]);
  const [compareScenario,setCompareScenario]=useState<number>();
  const [otherRuns,setOtherRuns]=useState<{scenarioId:number;runs:Run[]}>();
  const [error,setError]=useState<string>();
  const [compare,setCompare]=useState<number>();
  const [comparison,setComparison]=useState<{left:Results;right:Results;inputs:RunInputs;leftId:number;rightId:number}>();
  useEffect(()=>{let live=true;historyApi.entitlements().then(e=>live && setAccess(e)).catch(()=>{if(live)setError("Could not load access to comparisons and reports. Reload to try again.");});return()=>{live=false;};},[]);
  useEffect(()=>{let live=true;api.scenarios.list().then(plans=>live && setPlans(plans)).catch(()=>{});return()=>{live=false;};},[]);
  useEffect(()=>{
    if(compareScenario == null) return;
    let live=true;
    api.runs.list(compareScenario).then(runs=>live && setOtherRuns({scenarioId:compareScenario,runs})).catch((e:Error)=>live && setError(e.message));
    return()=>{live=false;};
  },[compareScenario]);
  useEffect(()=>{
    if(!pro || !selectedRunId || !compare || selectedRunId === compare) return;
    let live=true;
    historyApi.compare(selectedRunId,compare)
      .then(({left,right})=>{if(live)setComparison({left:left.results,right:right.results,inputs:right.inputs,leftId:selectedRunId,rightId:compare});})
      .catch((e:Error)=>live && setError(e.message));
    return()=>{live=false;};
  },[selectedRunId,compare,pro]);
  async function printReport(){
    if(!selectedRunId || !inputs || !pro) return;
    const target=window.open("","_blank");
    if(!target){setError("Allow the report window to open, then retry.");return;}
    try{
      const bundle=await historyApi.report(selectedRunId);
      target.document.open();target.document.write(reportHtml(bundle.results,bundle.inputs));target.document.close();
      target.focus();target.print();
    }catch(e){target.close();setError(e instanceof Error ? e.message:String(e));}
  }
  async function downloadInputs(){
    if(!selectedRunId) return;
    try{
      const archive=await historyApi.archive(selectedRunId);
      const url=URL.createObjectURL(new Blob([JSON.stringify(archive,null,2)],{type:"application/json"}));
      const link=document.createElement("a");link.href=url;link.download=`finplan-run-${selectedRunId}-inputs.json`;link.click();
      setTimeout(()=>URL.revokeObjectURL(url),1000);
    }catch(e){setError(e instanceof Error ? e.message:String(e));}
  }
  const displayed=comparison?.leftId === selectedRunId && comparison?.rightId === compare ? comparison : undefined;
  const available=compareScenario == null ? history : otherRuns?.scenarioId === compareScenario ? otherRuns.runs : [];
  const graph=inputs?.snapshot as {scenario?:{name?:string;start_date?:string;duration_years?:number}}|null;
  return <section aria-label="Saved run history" style={{border:"1px solid var(--color-divider)",padding:14,marginBottom:18}}>
    <label>Saved result <select aria-label="Saved result" value={selectedRunId ?? ""} onChange={e=>onSelectRun(Number(e.target.value))}>
      <option value="" disabled>Select a completed run</option>{history.filter(r=>r.status === "succeeded").map(r=><option key={r.id} value={r.id}>Run #{r.id} · {r.created_at} · {r.completed_iterations} iterations</option>)}
    </select></label>
    <p style={{fontSize:12}}>{inputs?.snapshot ? `${graph?.scenario?.name} · ${graph?.scenario?.start_date} · ${graph?.scenario?.duration_years} years · seed ${inputs.seed} · ${inputs.model_version}` : "Historical inputs unavailable. Dates use stored result checkpoints; age labels are unavailable."}</p>
    {inputs?.input_hash && <p style={{fontSize:11,overflowWrap:"anywhere"}}>Input fingerprint: {inputs.input_hash}</p>}
    <Button onClick={()=>void downloadInputs()} disabled={!inputs?.snapshot}>Download saved inputs</Button>
    {pro ? <div style={{display:"flex",gap:12,alignItems:"center"}}>
      <label>Comparison plan <select aria-label="Comparison plan" value={compareScenario ?? ""} onChange={e=>{setCompareScenario(e.target.value ? Number(e.target.value):undefined);setCompare(undefined);}}>
        <option value="">Current plan</option>{plans.filter(p=>p.id !== history[0]?.scenario_id).map(p=><option key={p.id} value={p.id}>{p.name}</option>)}
      </select></label>
      <label>Compare with <select aria-label="Compare with saved run" value={compare ?? ""} onChange={e=>setCompare(e.target.value ? Number(e.target.value):undefined)}>
        <option value="">Choose another run</option>{available.filter(r=>r.status === "succeeded" && r.id !== selectedRunId).map(r=><option key={r.id} value={r.id}>Run #{r.id} · {r.created_at}</option>)}
      </select></label><Button onClick={()=>void printReport()} disabled={!inputs}>Print / Save PDF report</Button>
    </div>:access?.access_mode === "subscription" ? <p>Pro includes saved-run comparisons and printable reports.</p>:null}
    {compare && compare !== selectedRunId && !displayed && <p role="status">Loading comparison…</p>}
    {displayed && inputs && <div>
      <p>Changed input sections: {!inputs.snapshot || !displayed.inputs.snapshot ? "Unavailable for runs without captured inputs" : changedInputSections(inputs,displayed.inputs).join(", ") || "None detected in captured definitions"}.</p>
      {comparisonWarnings(inputs,displayed.inputs).map(w=><p role="status" key={w}>{w}</p>)}
      <table style={{width:"100%",marginTop:14,textAlign:"left"}}><thead><tr><th>Measurement</th><th>Run #{selectedRunId}</th><th>Run #{compare}</th></tr></thead><tbody>
      <tr><th>Cash funding</th><td>{rate(displayed.left.stats.funding_success_rate)}</td><td>{rate(displayed.right.stats.funding_success_rate)}</td></tr>
      <tr><th>Positive ending net worth</th><td>{rate(displayed.left.stats.success_rate)}</td><td>{rate(displayed.right.stats.success_rate)}</td></tr>
      <tr><th>Actual iterations</th><td>{displayed.left.stats.num_iterations}</td><td>{displayed.right.stats.num_iterations}</td></tr>
      <tr><th>Mean ending net worth (nominal)</th><td>{money(displayed.left.stats.mean_final_net_worth)}</td><td>{money(displayed.right.stats.mean_final_net_worth)}</td></tr>
      <tr><th>Mean ending net worth (real)</th>{[displayed.left,displayed.right].map(r=><td key={r.run_id}>{r.real_net_worth ? `${money(r.real_net_worth.terminal.mean)} in ${r.real_net_worth.terminal.base_date} dollars` : "Not measured"}</td>)}</tr>
      <tr><th>Seed</th><td>{inputs.seed ?? "Not recorded"}</td><td>{displayed.inputs.seed ?? "Not recorded"}</td></tr>
      </tbody></table><p style={{fontSize:12}}>Cash funding includes cash shortfalls and event warnings. These estimates do not identify omitted spending or guarantee future outcomes.</p>
    </div>}
    {error && <p role="alert">{error}</p>}
  </section>;
}
