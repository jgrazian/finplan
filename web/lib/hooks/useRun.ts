"use client";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { planApiFor } from "@/lib/nav/api";
import { homeOf } from "@/lib/nav/url";
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
  /** Re-read the plan's runs: one was stored from outside (a run offloaded to the server). */
  refresh: () => void;
}
interface Loaded {
  /** `home:id`: an id is only unique within its home, so the home is part of the key. */
  key: string; history: Run[]; selected?: number; raw?: Results; inputs?: RunInputs;
  rawSeries?: Percentile; hash?: string; hashPending?: boolean; error?: string; loading: boolean;
}
export function useRun(scenario: Scenario | undefined, planRef: string | undefined): RunState {
  const id = scenario?.id;
  const home = homeOf(planRef);
  const key = id == null ? undefined : `${home}:${id}`;
  const activeKey = useRef(key);
  useLayoutEffect(() => { activeKey.current = key; }, [key]);
  const [loaded, setLoaded] = useState<Loaded>();
  const [revision, setRevision] = useState(0);
  const [listRevision, setListRevision] = useState(0);
  const [percentile, setPercentile] = useState<Percentile>("p50");
  const current = loaded?.key === key ? loaded : undefined;
  const history = current?.history ?? [];
  const run = preferredRun(history);
  const active = run != null && !isTerminal(run.status);
  const chosen = history.find(r => r.id === current?.selected && r.status === "succeeded") ?? history.find(r => r.status === "succeeded");
  const selectedRunId = chosen?.id;
  const update = useCallback((forKey:string, change: (value:Loaded)=>Loaded) => {
    setLoaded(previous => activeKey.current !== forKey ? previous : change(previous?.key === forKey ? previous : {key:forKey,history:[],loading:true}));
  }, []);
  // The home's api is looked up inside each effect rather than held: it is a
  // constant per home, and `home` is the dependency that says when it changes.
  useEffect(() => {
    if (id == null || key == null) return;
    let live = true;
    planApiFor(home).runs.list(id).then(history => {
      if (live) update(key, value => ({...value, history, loading: history.some(r=>r.status === "succeeded")}));
    }).catch((e:Error)=>live && update(key,value=>({...value,error:e.message,loading:false})));
    return ()=>{live=false;};
  },[id,home,key,listRevision,update]);
  // Re-read actual dependencies after every successful mutation, including edits
  // made in the same second and shared library changes. Never clear staleness on completion.
  useEffect(()=>{
    if (id == null || key == null) return;
    let live=true;
    planApiFor(home).scenarios.inputHash(id).then(({input_hash})=>live && update(key,v=>({...v,hash:input_hash,hashPending:false})))
      .catch((e:Error)=>live && update(key,v=>({...v,hashPending:true,error:e.message})));
    return ()=>{live=false;};
  },[id,home,key,scenario?.updated_at,revision,run?.status,update]);
  useEffect(()=>{
    if (id == null || key == null || selectedRunId == null) return;
    let live=true;
    const api=planApiFor(home);
    Promise.all([api.runs.results(selectedRunId,SERIES[percentile]),api.runs.inputs(selectedRunId)])
      .then(([raw,inputs])=>{
        if (live && raw.scenario_id === id && raw.run_id === selectedRunId) update(key,v=>({...v,raw,inputs,rawSeries:percentile,loading:false,error:undefined}));
      }).catch((e:Error)=>live && update(key,v=>({...v,error:e.message,loading:false})));
    return ()=>{live=false;};
  },[id,home,key,selectedRunId,percentile,update]);
  useEffect(()=>{
    if (id == null || key == null || !run || isTerminal(run.status)) return;
    let live=true;
    const timer=setInterval(async()=>{
      try {
        const next=await planApiFor(home).runs.get(run.id);
        if(live) update(key,v=>({...v,history:v.history.map(r=>r.id === next.id ? next : r),error:next.status === "failed" ? next.error_message ?? "Run failed" : v.error}));
      } catch(e) {if(live) update(key,v=>({...v,error:e instanceof Error ? e.message : String(e)}));}
    },700);
    return ()=>{live=false;clearInterval(timer);};
  },[id,home,key,run,update]);
  useEffect(()=>{
    if(run?.status === "failed") serverMonitor.runFailed({completed:run.completed_iterations,total:run.max_iterations ?? run.iterations,message:run.error_message,hasResults:current?.raw != null});
    else serverMonitor.runCleared();
  },[run,current?.raw]);
  const start=useCallback(async(effort:RunEffort)=>{
    if(id == null || key == null) return;
    try {
      const queued=await planApiFor(home).runs.create(id,{iterations:effort.iterations,converge:effort.converge,percentiles:STORED_PERCENTILES});
      update(key,v=>({...v,history:[queued,...v.history],selected:undefined,error:undefined,loading:false}));
      setRevision(v=>v+1);
    }catch(e){update(key,v=>({...v,error:e instanceof Error ? e.message : String(e),loading:false}));}
  },[id,home,key,update]);
  const cancel=useCallback(async()=>{
    if(id == null || key == null || !run) return;
    try{const next=await planApiFor(home).runs.cancel(run.id);update(key,v=>({...v,history:v.history.map(r=>r.id === next.id ? next:r)}));}
    catch(e){update(key,v=>({...v,error:e instanceof Error ? e.message:String(e)}));}
  },[id,home,key,run,update]);
  const selectRun=useCallback((selected:number)=>{if(key != null) update(key,v=>({...v,selected,error:undefined}));},[key,update]);
  const refresh=useCallback(()=>{
    if(key != null) update(key,v=>({...v,selected:undefined,error:undefined}));
    setListRevision(v=>v+1);
  },[key,update]);
  const markInputsChanged=useCallback(()=>{
    if(key != null) update(key,v=>({...v,hashPending:true}));
    setRevision(v=>v+1);
  },[key,update]);
  const raw=current?.raw;
  const inputs=current?.inputs;
  const source=raw && inputs ? snapshotScenario(inputs,raw) : undefined;
  const results=raw && source ? toResultsData(raw,source,planAxis(source)) : undefined;
  const stale=raw != null && (current?.hashPending === true || !inputs?.input_hash || inputs.input_hash !== current?.hash);
  const loading=(current?.loading ?? id != null) || (!current?.error && selectedRunId != null && (raw?.run_id !== selectedRunId || current?.rawSeries !== percentile));
  return {run,results,history,selectedRunId,selectRun,inputs,active,stale,markInputsChanged,loading,error:current?.error,percentile,setPercentile,start,cancel,refresh};
}
