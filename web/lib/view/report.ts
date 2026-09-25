import type { Results } from "../api/types";
import type { RealQuantilePoint } from "../api/generated/RealQuantilePoint";
import type { RunInputs } from "../api/generated/RunInputs";
export function escapeReport(value: unknown): string {
  return String(value ?? "Not recorded").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]!));
}
type Row = Record<string, unknown>;
type Graph = Record<string, unknown>;
const record = (v: unknown): Row => v && typeof v === "object" ? v as Row : {};
const rows = (v: unknown): Row[] => Object.values(record(v)).map(record);
const money = (v: unknown) => Number(v ?? 0).toLocaleString("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 });
const percent = (v: unknown) => v == null ? "Not measured — rerun" : `${(Number(v) * 100).toFixed(1)}%`;
const words = (v: unknown) => String(v ?? "Not recorded").replace(/([a-z])([A-Z])/g, "$1 $2");
const row = (key: unknown, value: unknown) => `<tr><th>${escapeReport(key)}</th><td>${escapeReport(value)}</td></tr>`;
const table = (body: string) => `<table>${body}</table>`;
function distribution(graph: Graph, id: unknown, depth = 0): string {
  const d = record(record(graph.distributions)[String(id)]);
  if (depth > 8) return "Nested distribution";
  const kind = String(d.kind ?? "No growth");
  if (kind === "Fixed") return `Fixed annual return ${percent(d.rate)}`;
  if (kind === "None") return "No growth";
  if (kind === "RegimeSwitching") return `Market regimes: bull (${distribution(graph, d.bull_id, depth + 1)}), bear (${distribution(graph, d.bear_id, depth + 1)})`;
  return [words(kind), d.mean != null ? `annual mean ${percent(d.mean)}` : null, d.std_dev != null ? `annual standard deviation ${percent(d.std_dev)}` : null, d.history_preset ? `historical series ${d.history_preset}` : null, d.block_size ? `block size ${d.block_size}` : null].filter(Boolean).join(" · ");
}
function assumptions(graph: Graph): string {
  const scenario = record(graph.scenario), assets = rows(graph.assets);
  const accounts = rows(graph.accounts).map(a => {
    const id = String(a.id), bank = record(record(graph.bank)[id]), investment = record(record(graph.investment)[id]), property = record(record(graph.property)[id]), liability = record(record(graph.liability)[id]);
    const holdings = rows(record(graph.positions)[id]).reduce((sum, p) => sum + Number(p.units) * Number(assets.find(a => a.id === p.asset_id)?.initial_price ?? 0), 0);
    const opening = Number(bank.cash_value ?? investment.cash_value ?? property.value ?? 0) + holdings - Number(liability.principal ?? 0);
    return row(a.name, `${words(investment.tax_status ?? a.flavor)} · opening ${money(opening)}${holdings ? `, including ${money(holdings)} in holdings` : ""}`);
  }).join("");
  const profiles = rows(graph.return_profiles).map(p => row(p.name, distribution(graph, p.distribution_id))).join("");
  const assetTable = assets.map(a => row(a.name, `Opening unit price ${money(a.initial_price)} · ${a.return_profile_id == null ? "No growth (unmapped)" : record(record(graph.return_profiles)[String(a.return_profile_id)]).name}${a.tracking_error ? ` · tracking error ${percent(a.tracking_error)}` : ""}`)).join("");
  const tax = record(graph.tax_config);
  const eventTable = rows(graph.events).map(e => {
    const trigger = record(record(graph.triggers)[String(record(graph.event_trigger)[String(e.id)])]);
    const effectIds = record(graph.event_effects)[String(e.id)] as number[] | undefined;
    const effects = (effectIds ?? []).map(id => {
      const effect = record(record(graph.effects)[String(id)]);
      const amount = record(record(graph.amounts)[String(effect.amount_id)]);
      return `${words(effect.kind)}${amount.kind === "Fixed" ? ` ${money(amount.value)}` : amount.kind ? ` (${words(amount.kind)})` : ""}`;
    });
    return row(e.name, `${e.enabled ? "Enabled" : "Disabled"} · ${words(trigger.kind)}${trigger.interval ? ` ${words(trigger.interval)}` : ""}${trigger.on_date ? ` ${trigger.on_date}` : ""}${trigger.age_years != null ? ` age ${trigger.age_years}` : ""} · ${effects.join(", ")}`);
  }).join("");
  return `<h2>Plan assumptions</h2>${table(row("Plan start", scenario.start_date) + row("Horizon", `${scenario.duration_years} years`) + row("Birth date", scenario.birth_date ?? "Not supplied"))}
    <h3>Opening accounts</h3>${accounts ? table(accounts) : "<p>No accounts recorded.</p>"}
    <h3>Assets and return assumptions</h3>${assetTable ? table(assetTable) : ""}${profiles ? table(profiles) : "<p>No return profiles recorded.</p>"}
    <h3>Inflation</h3><p>${escapeReport(graph.inflation_profile_name ?? "No inflation profile")} · ${escapeReport(graph.inflation_distribution_id == null ? "No inflation modeled" : distribution(graph, graph.inflation_distribution_id))}</p>
    <h3>Tax assumptions</h3>${graph.tax_config ? table(row("Profile and vintage label", tax.name) + row("State rate", percent(tax.state_rate)) + row("Capital gains rate", percent(tax.capital_gains_rate)) + row("Early withdrawal penalty", percent(tax.early_withdrawal_penalty_rate))) : "<p>No tax profile recorded.</p>"}
    ${rows(graph.tax_brackets).length ? `<p>Ordinary-income brackets: ${escapeReport(rows(graph.tax_brackets).map(b => `${money(b.threshold)} at ${percent(b.rate)}`).join("; "))}.</p>` : ""}
    <h3>Income, spending and life events</h3>${eventTable ? table(eventTable) : "<p>No events recorded. Missing spending can overstate cash funding.</p>"}
    <p>This summary describes the saved assumptions; download the saved inputs in FinPlan to inspect complete funding rules and nested event schedules.</p>`;
}
/** Final real net worth at every percentile the run measured, low to high. */
function terminalRow(point: RealQuantilePoint, baseDate: string): string {
  const all: Array<[number, number | null]> = [
    [5, point.p5],
    [10, point.p10],
    [25, point.p25],
    [50, point.p50],
    [75, point.p75],
    [90, point.p90],
    [95, point.p95],
  ];
  const ranks = all.filter((entry): entry is [number, number] => entry[1] != null);
  return row(
    `Final real net worth ${ranks.map(([p]) => `P${p}`).join(" / ")} (${baseDate} dollars)`,
    ranks.map(([, v]) => money(v)).join(" / "),
  );
}
export function reportHtml(results: Results, inputs: RunInputs): string {
  const graph = inputs.snapshot as Graph | null, scenario = record(graph?.scenario);
  const real = results.real_net_worth;
  const terminalPoint = real?.points.at(-1);
  const percentileRows = (results.stats.percentile_values ?? []).map(p => row(`Final nominal net worth P${Math.round(p.percentile * 100)}`, money(p.final_net_worth))).join("");
  return `<!doctype html><html><head><meta charset="utf-8"><title>FinPlan run ${results.run_id}</title><style>body{font:14px system-ui;max-width:900px;margin:32px auto;padding:24px;color:#172520;line-height:1.5}h1{font-size:28px}h2{margin-top:28px}table{width:100%;border-collapse:collapse}td,th{text-align:left;padding:8px;border-bottom:1px solid #bbb;vertical-align:top}th{width:38%}p{overflow-wrap:anywhere}@media print{body{margin:0}tr{break-inside:avoid}h2,h3{break-after:avoid}}</style></head><body>
  <h1>${escapeReport(scenario.name ?? "Historical plan")}</h1><p>FinPlan saved run #${results.run_id} · Generated ${escapeReport(new Date().toISOString())}</p>
  <p>This report describes the saved run's inputs. It does not incorporate later plan edits. Simulation outcomes depend on the assumptions below and are not guarantees.</p>
  <h2>Outcomes</h2>${table(row("Cash funding", percent(results.stats.funding_success_rate)) + row("Positive ending net worth", percent(results.stats.success_rate)) + row("Mean final net worth (nominal)", money(results.stats.mean_final_net_worth)) + percentileRows + (real ? row(`Mean final net worth (${real.terminal.base_date} dollars)`, money(real.terminal.mean)) : "") + (terminalPoint ? terminalRow(terminalPoint, real!.terminal.base_date) : "") + row("Lifetime taxes (nominal run summary)", money(results.stats.lifetime_taxes)))}
  <p>Cash funding checks settled cash shortfalls and event-processing warnings. It cannot detect omitted spending. Positive ending net worth measures terminal wealth and can coexist with earlier cash shortfalls. Percentiles describe simulated outcomes, not guarantees.</p>
  <h2>Selected path warnings</h2><p>Path ${escapeReport(results.series_id)}; these are representative-path diagnostics, not all-path failure frequencies.</p><ul>${results.warnings.map(w => `<li>${escapeReport(w.date)}: ${escapeReport(w.message)}</li>`).join("") || "<li>No warnings recorded for this path.</li>"}</ul>
  ${graph ? assumptions(graph) : "<h2>Saved assumptions</h2><p>Historical inputs were not captured. Assumptions cannot be reconstructed.</p>"}
  <h2>Reproducibility</h2>${table(row("Model version", inputs.model_version) + row("Seed", inputs.seed) + row("Iterations completed", results.stats.num_iterations) + row("Sampling rule", inputs.converge ? `Converging: minimum ${inputs.iterations}, ceiling ${inputs.max_iterations}` : `Fixed: ${inputs.iterations} iterations`) + row("Batch size / parallel batches", `${inputs.batch_size} / ${inputs.parallel_batches}`) + row("Stored representative percentiles", inputs.percentiles.map(p => `P${p * 100}`).join(", ")))}
  <p>Input fingerprint: ${escapeReport(inputs.input_hash)}.</p></body></html>`;
}
