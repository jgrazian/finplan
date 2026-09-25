"use client";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api } from "@/lib/api/client";
import { historyApi } from "@/lib/api/history";
import type { Results, Run, Scenario } from "@/lib/api/types";
import type { RunInputs } from "@/lib/api/generated/RunInputs";
import { SERIES, STORED_PERCENTILES, isTerminal } from "@/lib/api/types";
import { preferredRun } from "@/lib/run/freshness";
import { serverMonitor } from "@/lib/status/monitor";
import type { RunEffort } from "@/components/results";
import type { Percentile, ResultsData } from "@/lib/types";
import { planAxis } from "@/lib/view/axis";
import { snapshotScenario } from "@/lib/view/history";
import { toResultsData } from "@/lib/view/results";

export interface RunState {
  run: Run | undefined; results: ResultsData | undefined; active: boolean; stale: boolean;
  history: Run[]; selectedRunId: number | undefined; selectRun: (id:number) => void;
  inputs: RunInputs | undefined;
  markInputsChanged: () => void; loading: boolean; error: string | undefined;
  percentile: Percentile; setPercentile: (percentile: Percentile) => void;
  start: (effort: RunEffort) => Promise<void>; cancel: () => Promise<void>;
}
interface Loaded {
  scenarioId: number; history: Run[]; selected?: number; raw?: Results; inputs?: RunInputs;
  rawSeries?: Percentile; hash?: string; hashPending?: boolean; error?: string; loading: boolean;
}
export function useRun(scenario: Scenario | undefined): RunState {
  const id = scenario?.id;
  const activeId = useRef(id);
  useLayoutEffect(() => { activeId.current = id; }, [id]);
  const [loaded, setLoaded] = useState<Loaded>();
  const [revision, setRevision] = useState(0);
  const [percentile, setPercentile] = useState<Percentile>("p50");
  const current = loaded?.scenarioId === id ? loaded : undefined;
  const history = current?.history ?? [];
  const run = preferredRun(history);
  const active = run != null && !isTerminal(run.status);
  const chosen = history.find(r => r.id === current?.selected && r.status === "succeeded") ?? history.find(r => r.status === "succeeded");
  const selectedRunId = chosen?.id;
  const update = useCallback((scenarioId:number, change: (value:Loaded)=>Loaded) => {
    setLoaded(previous => activeId.current !== scenarioId ? previous : change(previous?.scenarioId === scenarioId ? previous : {scenarioId,history:[],loading:true}));
  }, []);
  useEffect(() => {
    if (id == null) return;
    let live = true;
    api.runs.list(id).then(history => {
      if (live) update(id, value => ({...value, history, loading: history.some(r=>r.status === "succeeded")}));
    }).catch((e:Error)=>live && update(id,value=>({...value,error:e.message,loading:false})));
    return ()=>{live=false;};
  },[id,update]);
  // Re-read actual dependencies after every successful mutation, including edits
  // made in the same second and shared library changes. Never clear staleness on completion.
  useEffect(()=>{
    if (id == null) return;
    let live=true;
    historyApi.hash(id).then(({input_hash})=>live && update(id,v=>({...v,hash:input_hash,hashPending:false})))
      .catch((e:Error)=>live && update(id,v=>({...v,hashPending:true,error:e.message})));
    return ()=>{live=false;};
  },[id,scenario?.updated_at,revision,run?.status,update]);
  useEffect(()=>{
    if (id == null || selectedRunId == null) return;
    let live=true;
    Promise.all([api.runs.results(selectedRunId,SERIES[percentile]),historyApi.inputs(selectedRunId)])
      .then(([raw,inputs])=>{
        if (live && raw.scenario_id === id && raw.run_id === selectedRunId) update(id,v=>({...v,raw,inputs,rawSeries:percentile,loading:false,error:undefined}));
      }).catch((e:Error)=>live && update(id,v=>({...v,error:e.message,loading:false})));
    return ()=>{live=false;};
  },[id,selectedRunId,percentile,update]);
  useEffect(()=>{
    if (id == null || !run || isTerminal(run.status)) return;
    let live=true;
    const timer=setInterval(async()=>{
      try {
        const next=await api.runs.get(run.id);
        if(live) update(id,v=>({...v,history:v.history.map(r=>r.id === next.id ? next : r),error:next.status === "failed" ? next.error_message ?? "Run failed" : v.error}));
      } catch(e) {if(live) update(id,v=>({...v,error:e instanceof Error ? e.message : String(e)}));}
    },700);
    return ()=>{live=false;clearInterval(timer);};
  },[id,run,update]);
  useEffect(()=>{
    if(run?.status === "failed") serverMonitor.runFailed({completed:run.completed_iterations,total:run.max_iterations ?? run.iterations,message:run.error_message,hasResults:current?.raw != null});
    else serverMonitor.runCleared();
  },[run,current?.raw]);
  const start=useCallback(async(effort:RunEffort)=>{
    if(id == null) return;
    try {
      const queued=await api.runs.create(id,{iterations:effort.iterations,converge:effort.converge,percentiles:STORED_PERCENTILES});
      update(id,v=>({...v,history:[queued,...v.history],selected:undefined,error:undefined,loading:false}));
      setRevision(v=>v+1);
    }catch(e){update(id,v=>({...v,error:e instanceof Error ? e.message : String(e),loading:false}));}
  },[id,update]);
  const cancel=useCallback(async()=>{
    if(id == null || !run) return;
    try{const next=await api.runs.cancel(run.id);update(id,v=>({...v,history:v.history.map(r=>r.id === next.id ? next:r)}));}
    catch(e){update(id,v=>({...v,error:e instanceof Error ? e.message:String(e)}));}
  },[id,run,update]);
  const selectRun=useCallback((selected:number)=>{if(id != null) update(id,v=>({...v,selected,error:undefined}));},[id,update]);
  const markInputsChanged=useCallback(()=>{
    if(id != null) update(id,v=>({...v,hashPending:true}));
    setRevision(v=>v+1);
  },[id,update]);
  const raw=current?.raw;
  const inputs=current?.inputs;
  const source=raw && inputs ? snapshotScenario(inputs,raw) : undefined;
  const results=raw && source ? toResultsData(raw,source,planAxis(source)) : undefined;
  const stale=raw != null && (current?.hashPending === true || !inputs?.input_hash || inputs.input_hash !== current?.hash);
  const loading=(current?.loading ?? id != null) || (!current?.error && selectedRunId != null && (raw?.run_id !== selectedRunId || current?.rawSeries !== percentile));
  return {run,results,history,selectedRunId,selectRun,inputs,active,stale,markInputsChanged,loading,error:current?.error,percentile,setPercentile,start,cancel};
}
