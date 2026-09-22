import test from "node:test";
import assert from "node:assert/strict";
import {snapshotScenario, comparisonWarnings, changedInputSections} from "../lib/view/history.ts";
import {reportHtml} from "../lib/view/report.ts";
const input={run_id:1,seed:42,iterations:100,converge:false,max_iterations:null,batch_size:100,parallel_batches:4,compute_mean:true,percentiles:[0.5],input_hash:"abc",model_version:"v1",snapshot:{scenario:{name:"Old plan",start_date:"2026-01-01",duration_years:20,birth_date:"1980-01-01"}}};
const raw={run_id:1,scenario_id:1,bands:[{dates:["2026-01-01","2046-01-01"]}],stats:{funding_success_rate:null,success_rate:0.8,num_iterations:100,mean_final_net_worth:100,lifetime_taxes:0},warnings:[],series_id:"0.5",real_net_worth:null} as never;
test("historical context uses captured dates; legacy runs use calendar years",()=>{
  assert.equal(snapshotScenario(input,raw).birth_date,"1980-01-01");
  assert.equal(snapshotScenario({...input,snapshot:null},raw).birth_date,null);
  assert.equal(snapshotScenario({...input,snapshot:null},raw).start_date,"2026-01-01");
});
test("comparisons disclose differences in seed horizon model and assumptions",()=>{
  const other={...input,seed:7,model_version:"v2",snapshot:{...input.snapshot,scenario:{...input.snapshot.scenario,duration_years:30}}};
  assert.equal(comparisonWarnings(input,other).length,3);
  assert.deepEqual(changedInputSections(input,other),["Plan dates and household"]);
  assert.deepEqual(changedInputSections(input,input),[]);
});
test("print report escapes user text, omits user ID, preserves missing funding",()=>{
  const html=reportHtml(raw,{...input,snapshot:{scenario:{name:"<script>alert(1)</script>",user_id:"PRIVATE"}}});
  assert.ok(html.includes("&lt;script&gt;"));assert.ok(!html.includes("<script>"));
  assert.ok(!html.includes("PRIVATE"));assert.ok(html.includes("Not measured — rerun"));
  assert.ok(html.includes("nominal"));assert.ok(html.includes("Cash funding"));
});
