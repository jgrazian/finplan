/**
 * The local backend (`PlanApi` over the store and the WASM engine), driven the
 * way the screens drive it, over the in-memory store and an in-process compute
 * pool. The engine is the real `.wasm`; see `tests/helpers/engine.mts`.
 */
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import type { Account, Asset, Event, NamedParameter, Profile, Scenario } from "../lib/api/types.ts";
const isTerminal = (status: string) => status === "succeeded" || status === "failed" || status === "canceled";
import { harness, requireEngine, setupAnswers, waitFor } from "./helpers/engine.mts";

if (requireEngine("local backend")) {
  const newPlan = (name = "Plan") => ({
    name,
    start_date: "2026-01-01",
    birth_date: "1985-06-15",
    duration_years: 30,
  });

  /** A harness with a library profile id and a plan made from guided setup. */
  async function withPlan(name = "Retirement") {
    const h = await harness();
    const profiles = await h.api.returnProfiles.list();
    const plan = await h.api.scenarios.get((await h.api.scenarios.setup(setupAnswers(profiles[0].id, { name }) as never)).scenario_id);
    return { ...h, profiles, plan };
  }

  /** The anonymized default plan the repository ships as test data, imported as a local plan. */
  async function withDefaultPlan() {
    const h = await harness();
    const graph = JSON.parse(
      readFileSync(new URL("../../crates/finplan_plan/testdata/default_snapshot.json", import.meta.url), "utf8"),
    ) as unknown;
    const archive = { format: "finplan.inputs", version: 3, plans: [graph] };
    const { scenario_ids } = await h.api.archives.import({ archive, name_prefix: "", request_id: "default", from_guest: false });
    return { ...h, plan: await h.api.scenarios.get(scenario_ids[0]) };
  }

  async function finished(h: Awaited<ReturnType<typeof harness>>, runId: number) {
    return waitFor(`run ${runId}`, async () => {
      const run = await h.api.runs.get(runId);
      return isTerminal(run.status) ? run : undefined;
    });
  }

  test("a plan is created, listed, renamed, copied and deleted, with its slug and its refusals", async () => {
    const h = await harness();
    assert.deepEqual(await h.api.scenarios.list(), []);

    const created = await h.api.scenarios.create(newPlan("First") as never);
    assert.equal(created.id, 1);
    assert.equal(created.slug, "l1");
    assert.equal(created.name, "First");
    assert.equal(created.status, "active");
    assert.equal(created.last_run_at, null);

    // Names are unique on a device, as on the server.
    await assert.rejects(h.api.scenarios.create(newPlan("First") as never), (e: { status: number; code: string }) => {
      assert.equal(e.status, 409);
      return true;
    });
    // Refused as the route refuses: a blank name, a bad date.
    await assert.rejects(h.api.scenarios.create({ ...newPlan("  ") } as never), (e: { status: number }) => e.status === 400);
    await assert.rejects(
      h.api.scenarios.create({ ...newPlan("x"), start_date: "nope" } as never),
      (e: { status: number }) => e.status === 400,
    );

    h.tick();
    const renamed = await h.api.scenarios.update(1, { name: "Renamed" });
    assert.equal(renamed.name, "Renamed");
    assert.notEqual(renamed.updated_at, created.updated_at);

    const copy = await h.api.scenarios.duplicate(1, "Copy");
    assert.equal(copy.id, 2);
    assert.equal(copy.slug, "l2");
    await assert.rejects(h.api.scenarios.duplicate(1, "Copy"), (e: { status: number }) => e.status === 409);
    await assert.rejects(h.api.scenarios.update(2, { name: "Renamed" }), (e: { status: number }) => e.status === 409);

    // Most recently updated first.
    h.tick();
    await h.api.scenarios.update(1, { description: "touched" });
    assert.deepEqual((await h.api.scenarios.list()).map((s: Scenario) => s.id), [1, 2]);

    await h.api.scenarios.remove(1);
    assert.deepEqual((await h.api.scenarios.list()).map((s: Scenario) => s.id), [2]);
    await assert.rejects(h.api.scenarios.get(1), (e: { status: number; code: string }) => e.status === 404 && e.code === "not_found");
    // Ids are never reused.
    assert.equal((await h.api.scenarios.create(newPlan("Third") as never)).id, 3);
  });

  test("the funding policy is set, read back, refused when bad, and cleared", async () => {
    const { api, plan } = await withPlan();
    // The fixture's guided setup funds from investments, which turns the policy on.
    assert.equal((await api.scenarios.get(plan.id)).funding?.strategy, "TaxEfficientEarly");
    const set = await api.scenarios.setFunding(plan.id, {
      funding: { strategy: "BracketFilling", bracket_ceiling: 0.22, exclude_accounts: [] },
    });
    assert.equal(set.funding?.strategy, "BracketFilling");
    assert.equal((await api.scenarios.get(plan.id)).funding?.bracket_ceiling, 0.22);
    await assert.rejects(
      api.scenarios.setFunding(plan.id, { funding: { strategy: "ProRata", bracket_ceiling: 0.1, exclude_accounts: [] } }),
      (e: { status: number }) => e.status === 400,
    );
    assert.equal((await api.scenarios.get(plan.id)).funding?.strategy, "BracketFilling");
    assert.equal((await api.scenarios.setFunding(plan.id, { funding: null })).funding, null);
  });

  test("guided setup makes a plan that compiles, and a retry makes no second one", async () => {
    const h = await harness();
    const [profile] = await h.api.returnProfiles.list();
    const answers = setupAnswers(profile.id, { request_id: "same-request" });
    const first = await h.api.scenarios.setup(answers as never);
    const again = await h.api.scenarios.setup(answers as never);
    assert.equal(again.scenario_id, first.scenario_id);
    assert.equal((await h.api.scenarios.list()).length, 1);
    await assert.rejects(
      h.api.scenarios.setup({ ...answers, annual_spending: 1 } as never),
      (e: { status: number }) => e.status === 400,
    );
    assert.equal((await h.api.scenarios.compile(first.scenario_id)).ok, true);
    assert.equal((await h.api.accounts.list(first.scenario_id)).length, 3);
  });

  test("every edit group writes through to the plan and answers with what the route does", async () => {
    const { api, plan, profiles } = await withPlan();
    const id = plan.id;
    const profile = profiles[0].id;

    // Accounts and their positions.
    const accounts = await api.accounts.list(id);
    const checking = accounts.find((a: Account) => a.name === "Checking") as Account;
    const bank = await api.accounts.create(id, {
      name: "Savings",
      flavor: "Bank",
      cash_value: 5_000,
      return_profile_id: profile,
    } as never);
    assert.equal(bank.name, "Savings");
    const renamed = await api.accounts.update(id, bank.id, { name: "Rainy day" } as never);
    assert.equal(renamed.name, "Rainy day");

    // Assets.
    const asset = await api.assets.create(id, { name: "VTI", initial_price: 250, return_profile_id: profile } as never);
    assert.equal(asset.name, "VTI");
    const updated = await api.assets.update(id, asset.id, { initial_price: 260 } as never);
    assert.equal(updated.initial_price, 260);

    const invest = (await api.accounts.create(id, {
      name: "Brokerage",
      flavor: "Investment",
      tax_status: "Taxable",
      cash_value: 0,
      cash_return_profile_id: profile,
      contribution_limit: null,
      contribution_period: null,
      plan_type: null,
      catch_up: [],
    } as never)) as Account;
    const lot = await api.accounts.addPosition(id, invest.id, { asset_id: asset.id, units: 10, cost_basis: 2_000 });
    assert.equal(lot.units, 10);
    const lot2 = await api.accounts.updatePosition(id, invest.id, lot.id, { units: 12 } as never);
    assert.equal(lot2.units, 12);
    assert.equal((await api.accounts.positions(id, invest.id)).length, 1);
    await api.accounts.removePosition(id, invest.id, lot.id);
    assert.equal((await api.accounts.positions(id, invest.id)).length, 0);

    // Parameters and events that follow them.
    const parameter = await api.parameters.create(id, { name: "Gift", value: { kind: "Money", value: 100 } } as never);
    assert.equal(parameter.name, "Gift");
    const changed = await api.parameters.update(id, parameter.id, { name: "Gift", value: { kind: "Money", value: 250 } } as never);
    assert.equal((changed as NamedParameter).name, "Gift");
    const event = await api.events.create(id, {
      name: "A gift",
      fires_once: true,
      enabled: true,
      trigger: { kind: "Date", on_date: "2030-01-01" },
      effects: [{ kind: "Income", to_account_id: checking.id, amount: { kind: "Fixed", value: 100 }, amount_mode: "Gross", income_type: "Taxable" }],
    } as never);
    assert.equal(event.name, "A gift");
    const validation = await api.expressions.validate(id, {
      effect: { kind: "Expense", from_account_id: checking.id, amount: { kind: "Expression", source: "$Gift * 2" } },
    } as never);
    assert.equal(typeof validation, "object");

    // Reorder: the list comes back in the order asked for.
    const events = await api.events.list(id);
    const reversed = events.map((e: Event) => e.id).reverse();
    await api.events.reorder(id, reversed);
    assert.deepEqual((await api.events.list(id)).map((e: Event) => e.id), reversed);
    const assets = (await api.assets.list(id)).map((a: Asset) => a.id).reverse();
    await api.assets.reorder(id, assets);
    assert.deepEqual((await api.assets.list(id)).map((a: Asset) => a.id), assets);

    // Deletes.
    await api.events.remove(id, event.id);
    await api.parameters.remove(id, parameter.id);
    await api.accounts.remove(id, bank.id);
    assert.equal((await api.events.list(id)).some((e: Event) => e.id === event.id), false);

    // Reads that are not lists.
    assert.equal((await api.scenarios.preflight(id)).issues !== undefined, true);
    assert.ok((await api.historyPresets()).length > 0);
  });

  test("a failed edit writes nothing, and a saved one is counted for the backup reminder", async () => {
    const { api, runtime, plan, tick } = await withPlan();
    const before = await runtime.planMeta(plan.id);
    assert.equal(before?.editsSinceExport, 0);
    const written = await api.scenarios.get(plan.id);

    tick();
    await assert.rejects(api.accounts.remove(plan.id, 9_999), (e: { status: number; code: string }) => e.status === 404);
    await assert.rejects(api.assets.create(plan.id, { name: "", initial_price: -1, return_profile_id: 1 } as never));
    await assert.rejects(api.assets.update(plan.id, 424242, { name: "x" } as never), (e: { status: number }) => e.status === 404);
    assert.deepEqual(await api.scenarios.get(plan.id), written);
    assert.deepEqual(await runtime.planMeta(plan.id), before);

    tick();
    await api.scenarios.update(plan.id, { description: "edited" });
    const after = await runtime.planMeta(plan.id);
    assert.equal(after?.editsSinceExport, 1);
    assert.notEqual(after?.updatedAt, before?.updatedAt);

    await runtime.markExported([plan.id]);
    assert.deepEqual((await runtime.planMeta(plan.id))?.editsSinceExport, 0);
    assert.ok((await runtime.planMeta(plan.id))?.lastExportedAt);
    assert.equal(await runtime.planMeta(424242), undefined);
  });

  test("a library edit reaches the plans that use the profile, and a profile in use is refused", async () => {
    const { api, plan, profiles } = await withPlan();
    const created = await api.returnProfiles.create({
      name: "Custom",
      description: null,
      asset_class: null,
      distribution: { kind: "Normal", mean: 0.05, std_dev: 0.1 },
    } as never);
    assert.equal(created.name, "Custom");
    const asset = await api.assets.create(plan.id, { name: "VTI", initial_price: 250, return_profile_id: created.id } as never);

    await api.returnProfiles.update(created.id, { name: "Custom 2" } as never);
    const listed = await api.returnProfiles.list();
    const used = listed.find((p: Profile) => p.id === created.id) as Profile;
    assert.equal(used.name, "Custom 2");
    assert.ok(used.used_by.length > 0, "usage is counted across the device's plans");
    // The plan's own copy of the profile follows.
    assert.equal((await api.assets.list(plan.id)).find((a: Asset) => a.id === asset.id)?.return_profile_id, created.id);
    await assert.rejects(api.returnProfiles.remove(created.id), (e: { status: number }) => e.status === 409 || e.status === 400);

    const order = (await api.returnProfiles.list()).map((p: Profile) => p.id).reverse();
    await api.returnProfiles.reorder(order);
    assert.deepEqual((await api.returnProfiles.list()).map((p: Profile) => p.id), order);

    const tax = await api.taxConfigs.create({
      name: "Flat",
      state_rate: 0.05,
      capital_gains_rate: 0.15,
      early_withdrawal_penalty_rate: 0.1,
      standard_deduction: 10_000,
      age_65_extra_deduction: 1_000,
      federal_brackets: [{ threshold: 0, rate: 0.2 }],
    } as never);
    await api.scenarios.update(plan.id, { tax_config_id: tax.id } as never);
    await api.taxConfigs.update(tax.id, { state_rate: 0.06 } as never);
    assert.equal((await api.taxConfigs.list()).find((t) => t.id === tax.id)?.state_rate, 0.06);
    assert.equal((await api.scenarios.get(plan.id)).tax_config_id, tax.id);
    // The inflation library.
    const inflation = await api.inflationProfiles.create({
      name: "Steady",
      description: null,
      asset_class: null,
      distribution: { kind: "Normal", mean: 0.03, std_dev: 0.01 },
    } as never);
    assert.equal(inflation.name, "Steady");
    await api.inflationProfiles.remove(inflation.id);
    assert.equal(profiles.length > 0, true);
  });

  test("a plan exports to an archive and imports as a new plan with its assumptions of its own", async () => {
    const { api, plan } = await withPlan("Original");
    const archive = await api.scenarios.archive(plan.id);
    assert.equal(archive.plans.length, 1);
    const preview = await api.archives.preview(archive);
    assert.deepEqual(preview.names, ["Original"]);

    // The same name is refused, as the server refuses it; a prefix makes room.
    await assert.rejects(
      api.archives.import({ archive, name_prefix: "", request_id: "r1", from_guest: false }),
      (e: { status: number }) => e.status === 409,
    );
    const imported = await api.archives.import({ archive, name_prefix: "Restored: ", request_id: "r2", from_guest: false });
    assert.equal(imported.scenario_ids.length, 1);
    const restored = await api.scenarios.get(imported.scenario_ids[0]);
    assert.equal(restored.name, "Restored: Original");
    assert.equal((await api.scenarios.compile(restored.id)).ok, true);
    // A retry with the same key returns the first import, and makes no plan.
    const retried = await api.archives.import({ archive, name_prefix: "Restored: ", request_id: "r2", from_guest: false });
    assert.deepEqual(retried, imported);
    await assert.rejects(
      api.archives.import({ archive, name_prefix: "Another: ", request_id: "r2", from_guest: false }),
      (e: { status: number }) => e.status === 409,
    );
    assert.equal((await api.scenarios.list()).length, 2);

    // Everything, and a file that is not an archive.
    assert.equal((await api.archives.exportAll()).plans.length, 2);
    await assert.rejects(
      api.archives.preview({ format: "other", version: 1, plans: [] }),
      (e: { status: number }) => e.status === 400,
    );
  });

  test("a run is queued at once, reports progress, and stores results the screens read", async () => {
    const h = await withPlan();
    const queued = await h.api.runs.create(h.plan.id, { iterations: 200, percentiles: [0.1, 0.5, 0.9] });
    assert.equal(queued.status, "queued");
    assert.equal(queued.scenario_id, h.plan.id);
    assert.equal(queued.iterations, 200);
    assert.ok(queued.seed !== null && queued.seed >= 0 && queued.seed <= 2 ** 53 - 1);
    assert.equal(queued.model_version, h.engine.model_version());
    assert.match(queued.input_hash ?? "", /^[0-9a-f]{64}$/);

    const done = await finished(h, queued.id);
    assert.equal(done.status, "succeeded");
    assert.equal(done.completed_iterations, 200);
    assert.ok(done.finished_at);

    // The hash the run kept is the one the plan has now, so freshness works.
    assert.equal((await h.api.scenarios.inputHash(h.plan.id)).input_hash, done.input_hash);

    const results = await h.api.runs.results(done.id);
    assert.equal(results.run_id, done.id);
    assert.equal(results.scenario_id, h.plan.id);
    assert.equal(results.stats.num_iterations, 200);
    // Each percentile path keeps the seed that replays it, as decimal text; the mean has none.
    for (const band of results.bands) {
      if (band.percentile === null) assert.equal(band.seed, null);
      else assert.match(band.seed ?? "", /^\d+$/);
    }
    const mean = await h.api.runs.results(done.id, "mean");
    assert.equal(mean.series_id, "mean");
    await assert.rejects(h.api.runs.results(done.id, "p50"), (e: { status: number }) => e.status === 400);

    const page = await h.api.runs.ledger(done.id, { category: "cash", limit: 5 });
    assert.ok(page.entries.length <= 5);
    const inputs = await h.api.runs.inputs(done.id);
    assert.equal(inputs.seed, queued.seed);
    assert.equal(inputs.input_hash, done.input_hash);
    assert.equal((inputs.snapshot as { scenario: { name: string } }).scenario.name, "Retirement");
    const report = await h.api.runs.report(done.id);
    assert.equal(report.results.run_id, done.id);
    const archive = (await h.api.runs.archive(done.id)) as { plans: unknown[] };
    assert.equal(archive.plans.length, 1);

    // The plan list carries the run's figures.
    const [row] = await h.api.scenarios.list();
    assert.equal(row.last_success_rate, results.stats.success_rate);
    assert.ok(row.last_run_at);

    // Editing the plan makes the run stale: its hash no longer matches.
    await h.api.scenarios.update(h.plan.id, { description: "changed" });
    assert.equal((await h.api.runs.get(done.id)).input_hash, done.input_hash);
    await h.api.accounts.create(h.plan.id, {
      name: "More", flavor: "Bank", cash_value: 1, return_profile_id: h.profiles[0].id,
    } as never);
    assert.notEqual((await h.api.scenarios.inputHash(h.plan.id)).input_hash, done.input_hash);

    // Same seed, same plan: the same numbers, whatever the pool did.
    const h2 = await withPlan();
    const again = await h2.api.runs.create(h2.plan.id, { iterations: 200, seed: queued.seed });
    await finished(h2, again.id);
    const results2 = await h2.api.runs.results(again.id);
    assert.deepEqual(results2.stats, results.stats);
  });

  test("the result depends on the batch plan and the seed, not on the number of workers", async () => {
    const small = await harness({ poolSize: 1 });
    const big = await harness({ poolSize: 4 });
    const stats = [];
    for (const h of [small, big]) {
      const [profile] = await h.api.returnProfiles.list();
      const { scenario_id } = await h.api.scenarios.setup(setupAnswers(profile.id) as never);
      const run = await h.api.runs.create(scenario_id, { iterations: 400, seed: 99 });
      await finished(h, run.id);
      stats.push((await h.api.runs.results(run.id)).stats);
    }
    assert.deepEqual(stats[0], stats[1]);
  });

  test("a converging run goes round by round until its metric settles", async () => {
    const h = await withPlan();
    const queued = await h.api.runs.create(h.plan.id, { iterations: 100, converge: true });
    assert.equal(queued.converge, true);
    assert.ok((queued.max_iterations ?? 0) >= 100);
    const done = await finished(h, queued.id);
    assert.equal(done.status, "succeeded");
    const results = await h.api.runs.results(done.id);
    assert.ok(results.stats.num_iterations >= 100);
    assert.ok(results.stats.converged !== undefined);
  });

  test("a bad run is refused when it is asked for, and a cancelled one stops", async () => {
    const h = await withPlan();
    await assert.rejects(h.api.runs.create(h.plan.id, { iterations: 0 }), (e: { status: number }) => e.status === 400);
    await assert.rejects(h.api.runs.create(h.plan.id, { iterations: 10, seed: -5 }), (e: { status: number }) => e.status === 400);
    await assert.rejects(h.api.runs.create(404, { iterations: 10 }), (e: { status: number }) => e.status === 404);
    assert.equal((await h.api.runs.list(h.plan.id)).length, 0, "a refused run leaves nothing behind");

    const long = await h.api.runs.create(h.plan.id, { iterations: 100_000 });
    // Let it start, then cancel.
    await waitFor("it to run", async () => ((await h.api.runs.get(long.id)).status === "running" ? true : undefined));
    const cancelled = await h.api.runs.cancel(long.id);
    assert.equal(cancelled.status, "canceled");
    await new Promise((resolve) => setTimeout(resolve, 100));
    const after = await h.api.runs.get(long.id);
    assert.equal(after.status, "canceled");
    assert.ok(after.completed_iterations < 100_000);
    await assert.rejects(h.api.runs.results(long.id), (e: { status: number }) => e.status === 409);
    // Cancelling a finished run changes nothing.
    assert.equal((await h.api.runs.cancel(long.id)).status, "canceled");

    // The pool still works afterwards.
    const next = await h.api.runs.create(h.plan.id, { iterations: 50 });
    assert.equal((await finished(h, next.id)).status, "succeeded");
  });

  test("only the newest five runs of a plan are kept, and deleting a plan deletes its runs", async () => {
    const h = await withPlan();
    const ids: number[] = [];
    for (let i = 0; i < 7; i++) {
      const run = await h.api.runs.create(h.plan.id, { iterations: 20 });
      ids.push(run.id);
      await finished(h, run.id);
    }
    const kept = await h.api.runs.list(h.plan.id);
    assert.deepEqual(kept.map((r) => r.id), ids.slice(2).reverse());
    await assert.rejects(h.api.runs.get(ids[0]), (e: { status: number }) => e.status === 404);
    await assert.rejects(h.api.runs.results(ids[1]), (e: { status: number }) => e.status === 404);

    await h.api.runs.remove(ids[6]);
    assert.equal((await h.api.runs.list(h.plan.id)).length, 4);
    await h.api.scenarios.remove(h.plan.id);
    await assert.rejects(h.api.runs.get(ids[5]), (e: { status: number }) => e.status === 404);
  });

  test("a run computed on the server is stored as a run of this plan with engine server", async () => {
    const h = await withPlan();
    // Any real RunResults will do for the stand-in: make one locally and treat it as the server's.
    const local = await h.api.runs.create(h.plan.id, { iterations: 50 });
    await finished(h, local.id);
    const stored = await h.store.transact("r", (tx) => tx.getResults(local.id));
    const shot = await h.runtime.snapshot(h.plan.id);
    const saved = await h.runtime.saveServerRun(h.plan.id, {
      settings: { iterations: 50, seed: 3 },
      inputHash: shot.hash,
      modelVersion: shot.modelVersion,
      results: JSON.parse(stored as string),
    });
    assert.equal(saved.status, "succeeded");
    assert.equal(saved.input_hash, shot.hash);
    assert.equal((await h.api.runs.results(saved.id)).stats.num_iterations, 50);
    const record = await h.store.transact("r", (tx) => tx.getRun(saved.id));
    assert.equal(record?.engine, "server");
  });

  test("analyses run in a worker: a sweep reports progress and is kept, and a cancel ends it", async () => {
    const h = await withPlan();
    const parameters = await h.api.analysis.parameters(h.plan.id);
    const spending = parameters.find((p) => p.name === "Monthly spending");
    assert.ok(spending, "guided setup's spending is a parameter");
    assert.equal(await h.api.analysis.cachedSweep(h.plan.id), null);

    const started = await h.api.analysis.start(h.plan.id, {
      kind: "sweep",
      iterations: 25,
      axes: [{ parameter_id: spending.id, min: 3_000, max: 6_000, steps: 3 }],
    } as never);
    assert.equal(started.kind, "sweep");
    assert.ok(started.total > 0);
    const job = await waitFor("the sweep", async () => {
      const current = await h.api.analysis.get(started.id);
      return current.status === "running" || current.status === "queued" ? undefined : current;
    });
    assert.equal(job.status, "succeeded");
    assert.equal(job.completed, job.total);
    const outcome = await h.api.analysis.results(started.id);
    assert.equal(outcome.kind, "sweep");
    assert.equal((outcome as { cells: unknown[] }).cells.length, 3);

    const cached = await h.api.analysis.cachedSweep(h.plan.id);
    assert.equal(cached?.scenario_id, h.plan.id);
    assert.equal(cached?.results.cells.length, 3);
    assert.equal(cached?.layout, null);
    await h.api.analysis.saveSweepLayout(h.plan.id, [{ kind: "heatmap" }]);
    assert.deepEqual((await h.api.analysis.cachedSweep(h.plan.id))?.layout, [{ kind: "heatmap" }]);

    // Refused as the server refuses: nothing to vary, nothing asked.
    await assert.rejects(
      h.api.analysis.start(h.plan.id, { kind: "sweep", axes: [] } as never),
      (e: { status: number }) => e.status === 400,
    );
    await assert.rejects(h.api.analysis.results(9_999), (e: { status: number }) => e.status === 404);

    const big = await h.api.analysis.start(h.plan.id, {
      kind: "sweep",
      iterations: 2_000,
      axes: [{ parameter_id: spending.id, min: 3_000, max: 6_000, steps: 12 }],
    } as never);
    const cancelled = await h.api.analysis.cancel(big.id);
    assert.equal(cancelled.status, "canceled");
    await assert.rejects(h.api.analysis.results(big.id), (e: { status: number }) => e.status === 409);
  });

  test("a sweep split across the pool's workers is the sweep one worker makes", async () => {
    const outcomes: unknown[] = [];
    for (const poolSize of [1, 2, 3]) {
      const h = await harness({ poolSize });
      const profiles = await h.api.returnProfiles.list();
      const { scenario_id } = await h.api.scenarios.setup(setupAnswers(profiles[0].id) as never);
      const spending = (await h.api.analysis.parameters(scenario_id)).find((p) => p.name === "Monthly spending");
      assert.ok(spending);
      const started = await h.api.analysis.start(scenario_id, {
        kind: "sweep",
        iterations: 25,
        axes: [{ parameter_id: spending.id, min: 3_000, max: 6_000, steps: 5 }],
      } as never);
      const job = await waitFor("the sweep", async () => {
        const current = await h.api.analysis.get(started.id);
        return current.status === "running" || current.status === "queued" ? undefined : current;
      });
      assert.equal(job.status, "succeeded", `${poolSize} workers`);
      assert.equal(job.completed, job.total, `${poolSize} workers`);
      outcomes.push(await h.api.analysis.results(started.id));
    }
    assert.deepEqual(outcomes[1], outcomes[0]);
    assert.deepEqual(outcomes[2], outcomes[0]);
  });

  test("a quick what-if answers in the call, honours an abort, and applies to the plan or a copy", async () => {
    const h = await withPlan();
    const layers = [{ kind: "market-shock", age: 50, drop: 0.3 }];
    const outcome = await h.api.whatIf.quick(h.plan.id, { layers, iterations: 50 } as never);
    assert.equal(outcome.steps.length, 2);

    const controller = new AbortController();
    const asked = h.api.whatIf.quick(h.plan.id, { layers, iterations: 500 } as never, controller.signal);
    controller.abort();
    await assert.rejects(asked, (e: Error) => e.name === "AbortError");

    // The stack is kept per plan.
    assert.deepEqual(await h.api.whatIf.get(h.plan.id), { entries: [] });
    const stack = { entries: [{ id: "a", enabled: true, layer: layers[0] }] };
    await h.api.whatIf.save(h.plan.id, stack as never);
    assert.deepEqual(await h.api.whatIf.get(h.plan.id), stack);
    await assert.rejects(
      h.api.whatIf.save(h.plan.id, { entries: [{ id: "", enabled: true, layer: layers[0] }] } as never),
      (e: { status: number }) => e.status === 400,
    );

    const before = (await h.api.events.list(h.plan.id)).length;
    const copy = await h.api.whatIf.apply(h.plan.id, { layers, new_scenario_name: "With a crash" } as never);
    assert.equal(copy.name, "With a crash");
    assert.equal((await h.api.events.list(h.plan.id)).length, before);
    assert.equal((await h.api.events.list(copy.id)).length, before + 1);
    assert.deepEqual(await h.api.whatIf.get(h.plan.id), stack, "a copy leaves the stack");

    const applied = await h.api.whatIf.apply(h.plan.id, { layers } as never);
    assert.equal(applied.id, h.plan.id);
    assert.equal((await h.api.events.list(h.plan.id)).length, before + 1);
    assert.deepEqual(await h.api.whatIf.get(h.plan.id), { entries: [] }, "applying clears the stack");
  });

  test("drawdown replays a run's median path under each strategy, compares them, and refuses a run with no seed", async () => {
    const h = await withDefaultPlan();
    const queued = await h.api.runs.create(h.plan.id, { iterations: 60, seed: 5, percentiles: [0.1, 0.5, 0.9] });
    await finished(h, queued.id);

    const body = await h.api.runs.drawdown(queued.id);
    assert.equal(body.choices.length, 7);
    assert.equal(body.choices[0].choice.kind, "AsPlanned");
    assert.equal(body.choices[0].overlay, false);
    assert.equal(body.choices[1].overlay, true);
    assert.equal(body.retirement.source, "income");
    const years = body.choices[1].years;
    assert.ok(years.length > 0);
    assert.equal(years[0].withdrawals.length, body.accounts.length);
    // The seed is the median band's, and the rows balance.
    const bands = (await h.api.runs.results(queued.id)).bands;
    assert.equal(body.seed, bands.find((b) => b.percentile === 0.5)?.seed);
    for (const y of years) {
      const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
      const inflow = sum(y.income) + sum(y.withdrawals) + y.cash + y.shortfall;
      assert.ok(Math.abs(inflow - (y.spending + y.withdrawal_taxes + y.surplus)) <= 1);
    }

    const one = await h.api.runs.drawdown(queued.id, {
      strategies: [{ kind: "Strategy", strategy: "TaxFreeFirst" }],
      retirement_year: 2050,
    });
    assert.equal(one.retirement.source, "request");
    assert.equal(one.choices[0].years[0].year, 2050);
    await assert.rejects(
      h.api.runs.drawdown(queued.id, { strategies: [] }),
      (e: { status: number }) => e.status === 400,
    );

    const compared = await h.api.runs.drawdownCompare(queued.id, {
      request: { strategies: [{ kind: "AsPlanned" }, { kind: "Strategy", strategy: "ProRata" }] },
      iterations: 25,
    });
    assert.equal(compared.rows.length, 2);
    assert.equal(compared.iterations, 25);
    await assert.rejects(
      h.api.runs.drawdownCompare(queued.id, { iterations: 100_000 }),
      (e: { status: number }) => e.status === 400,
    );
    const controller = new AbortController();
    const asked = h.api.runs.drawdownCompare(queued.id, { iterations: 500 }, controller.signal);
    controller.abort();
    await assert.rejects(asked, (e: Error) => e.name === "AbortError");

    // A run kept without seeds (stored before they were) asks to be run again.
    const stored = JSON.parse((await h.store.transact("r", (tx) => tx.getResults(queued.id))) as string);
    for (const path of stored.paths) delete path.seed;
    const shot = await h.runtime.snapshot(h.plan.id);
    const old = await h.runtime.saveServerRun(h.plan.id, {
      settings: { iterations: 60, seed: 5 },
      inputHash: shot.hash,
      modelVersion: shot.modelVersion,
      results: stored,
    });
    for (const call of [() => h.api.runs.drawdown(old.id), () => h.api.runs.drawdownCompare(old.id)]) {
      await assert.rejects(call(), (e: { status: number; message: string }) => {
        assert.equal(e.status, 409);
        assert.equal(e.message, "Run the plan again to see drawdown.");
        return true;
      });
    }
    await assert.rejects(h.api.runs.drawdown(9999), (e: { status: number }) => e.status === 404);
  });

  test("applying a strategy to the plan sets the funding policy and every strategy sweep", async () => {
    const h = await withDefaultPlan();
    const strategies = async () => {
      const found: string[] = [];
      for (const event of await h.api.events.list(h.plan.id)) {
        for (const effect of event.effects) {
          const sources = effect.kind === "Sweep" ? effect.sources : null;
          if (sources?.mode === "Strategy") found.push(sources.strategy);
        }
      }
      return found;
    };
    assert.deepEqual(await strategies(), ["PenaltyAware", "PenaltyAware"]);
    await h.api.scenarios.setFunding(h.plan.id, { funding: { strategy: "TaxFreeFirst", exclude_accounts: [] } });
    assert.deepEqual(await strategies(), ["PenaltyAware", "PenaltyAware"], "off unless asked");
    const set = await h.api.scenarios.setFunding(h.plan.id, {
      funding: { strategy: "BracketFilling", bracket_ceiling: 0.22, exclude_accounts: [] },
      align_sweeps: true,
    });
    assert.equal(set.funding?.strategy, "BracketFilling");
    assert.deepEqual(await strategies(), ["BracketFilling", "BracketFilling"]);
  });

  test("the review writes rule notes about a run, keeps dismissals, and applies a path", async () => {
    const h = await withDefaultPlan();
    assert.equal(await h.runtime.review.get(h.plan.id), null);
    await assert.rejects(h.runtime.review.run(h.plan.id, null), (e: { status: number }) => e.status === 409);

    const run = await h.api.runs.create(h.plan.id, { iterations: 100 });
    await finished(h, run.id);
    const review = await h.runtime.review.run(h.plan.id, null);
    assert.equal(review.run_id, run.id);
    assert.equal(review.ai, null);
    assert.ok(review.suggestions.length > 0);
    assert.ok(review.suggestions.every((s) => s.source === "rules" && s.status === "open"));
    assert.deepEqual(await h.runtime.review.get(h.plan.id), review);

    // Set one aside; it stays set aside across the next review.
    const [first] = review.suggestions;
    const dismissed = await h.runtime.review.dismiss(h.plan.id, first.id, "dismissed");
    assert.equal(dismissed.status, "dismissed");
    assert.equal((await h.runtime.review.get(h.plan.id))?.suggestions.find((s) => s.id === first.id)?.status, "dismissed");
    const again = await h.runtime.review.run(h.plan.id, run.id);
    assert.equal(again.suggestions.length, review.suggestions.length - 1);
    // Taking it back reopens the note in the stored review.
    await assert.rejects(h.runtime.review.reopen(h.plan.id, 9_999), (e: { status: number }) => e.status === 404);

    // Apply the first path of the first note that has one, then it is applied and the plan moved.
    const withPath = again.suggestions.find((s) => s.paths.length > 0);
    assert.ok(withPath, "the default plan draws a note with a course of action");
    const path = withPath.paths.find((p) => p.recommended) ?? withPath.paths[0];
    const hash = (await h.api.scenarios.inputHash(h.plan.id)).input_hash;
    const applied = await h.runtime.review.apply(h.plan.id, withPath.id, {
      path: path.key,
      through_step: null,
      to: "plan",
      name: null,
    });
    assert.equal(applied.scenario_id, h.plan.id);
    assert.equal(applied.suggestion.status, "applied");
    assert.ok(applied.suggestion.paths.find((p) => p.key === path.key)?.steps.every((s) => s.applied));
    assert.notEqual((await h.api.scenarios.inputHash(h.plan.id)).input_hash, hash);
    // Once applied, the note cannot be applied again.
    await assert.rejects(
      h.runtime.review.apply(h.plan.id, withPath.id, { path: path.key, through_step: null, to: "plan", name: null }),
      (e: { status: number }) => e.status === 409,
    );
  });

  test("the estimate comes from the calibration, and a calibration is measured once", async () => {
    const h = await harness({ poolSize: 4, hardware: { cores: 8, memoryGb: 8 } });
    const [profile] = await h.api.returnProfiles.list();
    const { scenario_id } = await h.api.scenarios.setup(setupAnswers(profile.id) as never);

    const before = await h.runtime.estimateRun(scenario_id, { iterations: 1000 });
    assert.equal(before.calibrated, false);
    assert.equal(before.constrained, false);

    await h.local.start();
    const after = await h.runtime.estimateRun(scenario_id, { iterations: 1000 });
    assert.equal(after.calibrated, true);
    assert.equal(after.workers, 4);
    assert.ok(after.seconds > 0 && Number.isFinite(after.seconds));
    // More iterations take proportionally longer; a converging run is expected to take longer than its minimum.
    const bigger = await h.runtime.estimateRun(scenario_id, { iterations: 4000 });
    assert.ok(Math.abs(bigger.seconds / after.seconds - 4) < 0.01);
    const converging = await h.runtime.estimateRun(scenario_id, { iterations: 1000, converge: true });
    assert.ok(converging.seconds > after.seconds);
    // A run that fits in one batch cannot use more than one worker.
    const tiny = await h.runtime.estimateRun(scenario_id, { iterations: 1000, parallel_batches: 1 });
    assert.equal(tiny.workers, 1);
    assert.ok(Math.abs(tiny.seconds / after.seconds - 4) < 0.01);

    // Measured once: the stored rate is what later estimates use.
    const stored = await h.store.transact("r", (tx) => tx.getMeta<{ rate: number }>("calibration"));
    assert.ok(stored && stored.rate > 0);
    assert.equal(await h.local.calibrate().then((rate) => typeof rate), "number");

    const weak = await harness({ hardware: { cores: 2 } });
    const [p2] = await weak.api.returnProfiles.list();
    const planned = await weak.api.scenarios.setup(setupAnswers(p2.id) as never);
    assert.equal((await weak.runtime.estimateRun(planned.scenario_id, { iterations: 100 })).constrained, true);
    const lowMemory = await harness({ hardware: { cores: 8, memoryGb: 2 } });
    const [p3] = await lowMemory.api.returnProfiles.list();
    const lowPlan = await lowMemory.api.scenarios.setup(setupAnswers(p3.id) as never);
    assert.equal((await lowMemory.runtime.estimateRun(lowPlan.scenario_id, { iterations: 100 })).constrained, true);
  });

  test("a run a closed page left half-done is marked failed at startup", async () => {
    const h = await withPlan();
    await h.store.transact("rw", async (tx) => {
      const id = await tx.nextId("run");
      await tx.putRun({
        id, plan_id: h.plan.id, status: "running",
        settings: { iterations: 10, percentiles: [0.5], seed: 1, batch_size: 10, parallel_batches: 1, compute_mean: false, converge: false },
        iterations: 10, max_iterations: null, completed_iterations: 3, seed: 1, input_hash: null, model_version: null,
        engine: "wasm", error_message: null, created_at: "2026-10-03 11:00:00", started_at: "2026-10-03 11:00:01",
        finished_at: null, snapshot: null, success_rate: null,
      });
    });
    await h.local.start();
    const [run] = await h.api.runs.list(h.plan.id);
    assert.equal(run.status, "failed");
    assert.match(run.error_message ?? "", /closed/);
  });
}
