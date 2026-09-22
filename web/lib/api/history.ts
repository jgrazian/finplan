import type { RunReport } from "./generated/RunReport";
import type { RunComparison } from "./generated/RunComparison";
import { http } from "./http";
import type { RunInputs } from "./generated/RunInputs";
import type { Entitlements } from "./generated/Entitlements";
export type ReportBundle = RunReport;
export const historyApi = {
  report: (id:number)=>http.get<ReportBundle>(`/runs/${id}/report`),
  compare: (left:number,right:number)=>http.post<RunComparison>("/run-comparisons",{left_run_id:left,right_run_id:right}),
  archive:(id:number)=>http.get<unknown>(`/runs/${id}/archive`),
  inputs: (id: number) => http.get<RunInputs>(`/runs/${id}/inputs`),
  hash: (id: number) => http.get<{ input_hash: string }>(`/scenarios/${id}/input-hash`),
  entitlements: () => http.get<Entitlements>("/billing/entitlements"),
};
