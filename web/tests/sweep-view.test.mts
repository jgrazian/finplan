import assert from "node:assert/strict";
import { test } from "node:test";
import type { SweepAxis, SweepCell, SweepResults } from "../lib/api/types.ts";
import {
  DEFAULT_AZIMUTH,
  DEFAULT_ELEVATION,
  MAX_ELEVATION,
  METRICS,
  MIN_ELEVATION,
  RAMP,
  wrapAzimuth,
  axisNames,
  clampElevation,
  defaultGraphs,
  graphTitle,
  graphView,
  heldSummary,
  metric,
  needsY,
  newGraph,
  parseLayout,
  reconcile,
  shade,
  sweepCsv,
  sweepSpace,
  type GraphSpec,
} from "../lib/view/sweep.ts";

/**
 * A 3 × 2 × 2 sweep: retirement age across, spending, and an equity return.
 * Every cell's success rate is a function of all three, so holding one of them
 * somewhere other than its default has to change what a graph draws.
 */
const AXES: SweepAxis[] = [
  {
    parameter_id: "event:1:age",
    label: "Retire · age",
    role: "age",
    kind: "age",
    values: [60, 62, 64],
  },
  {
    parameter_id: "event:2:amount",
    label: "Spending · amount",
    role: "amount",
    kind: "amount",
    values: [5_000, 9_000],
  },
  {
    parameter_id: "event:3:amount",
    label: "Windfall · amount",
    role: "amount",
    kind: "amount",
    values: [0, 100_000],
  },
];

/** Success rises with age and windfall, falls with spending. */
function rate(age: number, spend: number, windfall: number): number {
  return 0.6 + age * 0.05 + windfall * 0.1 - spend * 0.15;
}

function results(axes: SweepAxis[] = AXES, planIndices: number[] | null = [1, 0, 0]): SweepResults {
  const shape = axes.map((axis) => axis.values.length);
  const cells: SweepCell[] = [];
  // Row-major, last axis fastest — the order the server writes them in.
  const walk = (slot: number, indices: number[]) => {
    if (slot === shape.length) {
      const [age = 0, spend = 0, windfall = 0] = indices;
      cells.push({
        indices: [...indices],
        success_rate: rate(age, spend, windfall),
        funding_success_rate: null,
        p5: 1_000 * (age + 1),
        p50: 10_000 * (age + 1) - 2_000 * spend,
        p95: 100_000,
      });
      return;
    }
    for (let at = 0; at < shape[slot]; at++) walk(slot + 1, [...indices, at]);
  };
  walk(0, []);

  return {
    axes,
    cells,
    default_metric: null,
    plan: { success_rate: 0.8, funding_success_rate: null, p5: 1_000, p50: 8_000, p95: 90_000 },
    plan_indices: planIndices,
    iterations: 250,
  };
}

function lineOver(x: string, held: Record<string, number> = {}): GraphSpec {
  return { id: "g", kind: "line", metric: "success", x, y: undefined, held, wide: false };
}

test("a cell is addressed by its position on every swept variable at once", () => {
  const space = sweepSpace(results());
  assert.deepEqual(space.shape, [3, 2, 2]);
  assert.equal(space.points, 12);
  assert.equal(space.at([2, 1, 0])?.success_rate, rate(2, 1, 0));
  assert.equal(space.at([0, 0, 1])?.success_rate, rate(0, 0, 1));
  // Off the grid, and the wrong number of coordinates, are both "not measured"
  // rather than a neighbouring cell.
  assert.equal(space.at([3, 0, 0]), undefined);
  assert.equal(space.at([0, 0]), undefined);
});

test("a graph draws its own two variables and holds the rest at the plan", () => {
  const space = sweepSpace(results());
  const view = graphView(space, {
    id: "g",
    kind: "heatmap",
    metric: "success",
    x: "event:1:age",
    y: "event:2:amount",
    held: {},
    wide: false,
  })!;

  assert.equal(view.xAxis.parameter_id, "event:1:age");
  assert.equal(view.yAxis?.parameter_id, "event:2:amount");
  // The windfall is not on an axis, so it sits where the plan sits: index 0.
  assert.deepEqual(
    view.held.map((h) => [h.axis.parameter_id, h.index, h.atPlan]),
    [["event:3:amount", 0, true]],
  );
  assert.equal(view.sample(2, 1), rate(2, 1, 0));
  assert.equal(view.planX, 1);
  assert.equal(view.planY, 0);
});

test("pinning a held variable slices the sweep somewhere other than the plan", () => {
  const space = sweepSpace(results());
  const atPlan = graphView(space, lineOver("event:1:age"))!;
  const pinned = graphView(space, lineOver("event:1:age", { "event:3:amount": 1 }))!;

  assert.equal(atPlan.sample(0), rate(0, 0, 0));
  assert.equal(pinned.sample(0), rate(0, 0, 1));
  assert.equal(
    pinned.held.find((h) => h.axis.parameter_id === "event:3:amount")?.atPlan,
    false,
  );
  assert.match(heldSummary(pinned), /pinned/);
  // The two amounts are named in full, so a card says which one it is holding.
  assert.match(heldSummary(pinned), /Spending · amount/);
  assert.match(heldSummary(pinned), /Windfall · amount/);
});

test("a plan off the swept range holds the middle step rather than the first", () => {
  const space = sweepSpace(results(AXES, null));
  const view = graphView(space, lineOver("event:1:age"))!;
  assert.equal(space.planIndices, undefined);
  assert.equal(view.planX, undefined);
  // Three-step axes have a middle; the two-step ones fall to their lower step.
  assert.deepEqual(
    view.held.map((h) => h.index),
    [0, 0],
  );
  assert.equal(view.held.every((h) => h.atPlan === false), true);
});

test("a graph naming a variable this sweep does not carry resolves to nothing", () => {
  const space = sweepSpace(results());
  assert.equal(graphView(space, lineOver("event:9:amount")), undefined);
});

test("every metric is read off the same cells", () => {
  const space = sweepSpace(results());
  const success = graphView(space, { ...lineOver("event:1:age"), metric: "success" })!;
  const p50 = graphView(space, { ...lineOver("event:1:age"), metric: "p50" })!;

  assert.equal(success.sample(2), rate(2, 0, 0));
  assert.equal(p50.sample(2), 30_000);
  // Funding success was never measured, so it draws nothing rather than zero.
  const funding = graphView(space, { ...lineOver("event:1:age"), metric: "funding" })!;
  assert.equal(funding.sample(2), undefined);
});

test("a band is opened out so a flat sweep is not stretched into a story", () => {
  const flat = metric("success").span([0.98, 0.98, 0.981]);
  assert.ok(flat.high - flat.low >= 0.0999, `${flat.low}–${flat.high}`);
  assert.ok(flat.high <= 1 && flat.low >= 0);

  // A rate band never leaves 0–1, even when the values sit against the ceiling.
  const ceiling = metric("success").span([1, 1]);
  assert.equal(ceiling.high, 1);
  assert.ok(ceiling.low >= 0.89 && ceiling.low <= 0.9001, `${ceiling.low}`);

  const wide = metric("success").span([0.4, 0.95]);
  assert.deepEqual(wide, { low: 0.4, high: 0.95 });
});

test("the ramp runs dark to light across the band it was given", () => {
  const span = { low: 0, high: 1 };
  assert.equal(shade(span, 0), RAMP[0]);
  assert.equal(shade(span, 1), RAMP[RAMP.length - 1]);
  // Outside the band clamps rather than falling off the ends of the ramp.
  assert.equal(shade(span, -5), RAMP[0]);
  assert.equal(shade(span, 5), RAMP[RAMP.length - 1]);
});

function surfaceOver(extra: Partial<GraphSpec> = {}): GraphSpec {
  return {
    id: "s",
    kind: "surface",
    metric: "success",
    x: "event:1:age",
    y: "event:2:amount",
    held: {},
    wide: false,
    ...extra,
  };
}

test("a surface's colour restates its height until it is pointed elsewhere", () => {
  const space = sweepSpace(results());

  const plain = graphView(space, surfaceOver())!;
  assert.equal(plain.twoMeasures, false);
  assert.equal(plain.colour.id, "success");
  assert.deepEqual(plain.colourSpan, plain.span);
  assert.equal(plain.sampleColour(2, 1), plain.sample(2, 1));

  // Naming the metric it already draws is not a second measure.
  const same = graphView(space, surfaceOver({ colour: "success" }))!;
  assert.equal(same.twoMeasures, false);

  const both = graphView(space, surfaceOver({ colour: "p50" }))!;
  assert.equal(both.twoMeasures, true);
  assert.equal(both.metric.id, "success");
  assert.equal(both.colour.id, "p50");
  // Height and colour read different numbers out of the same cell, and each
  // gets the band its own values cover.
  assert.equal(both.sample(2, 1), rate(2, 1, 0));
  assert.equal(both.sampleColour(2, 1), 30_000 - 2_000);
  assert.notDeepEqual(both.colourSpan, both.span);
  assert.ok(both.colourSpan.high > 1, `${both.colourSpan.high}`);
});

test("only a surface splits the two channels", () => {
  const space = sweepSpace(results());
  // A heatmap has nothing but colour, so a colour of its own would be the
  // graph's whole subject quietly replaced.
  const heat = graphView(space, surfaceOver({ kind: "heatmap", colour: "p50" }))!;
  assert.equal(heat.twoMeasures, false);
  assert.equal(heat.colour.id, "success");
});

test("a surface remembers where it is looked at from, within reason", () => {
  const space = sweepSpace(results());
  const fresh = graphView(space, surfaceOver())!;
  assert.equal(fresh.azimuth, DEFAULT_AZIMUTH);
  assert.equal(fresh.elevation, DEFAULT_ELEVATION);

  const turned = graphView(space, surfaceOver({ azimuth: 215, elevation: 55 }))!;
  assert.equal(turned.azimuth, 215);
  assert.equal(turned.elevation, 55);

  // Flat on the horizon collapses the grid, straight down collapses the
  // height; neither is a surface, so neither is reachable.
  assert.equal(graphView(space, surfaceOver({ elevation: 0 }))!.elevation, MIN_ELEVATION);
  assert.equal(graphView(space, surfaceOver({ elevation: 90 }))!.elevation, MAX_ELEVATION);
  assert.equal(clampElevation(Number.NaN), DEFAULT_ELEVATION);

  // A drag past either end of the turn keeps going rather than stopping dead.
  assert.equal(wrapAzimuth(370), 10);
  assert.equal(wrapAzimuth(-30), 330);
  assert.equal(wrapAzimuth(-390), 330);
  assert.equal(graphView(space, surfaceOver({ azimuth: 420 }))!.azimuth, 60);
  assert.equal(wrapAzimuth(Number.NaN), DEFAULT_AZIMUTH);
});

test("a fresh sweep opens on a grid of the first two variables and a line", () => {
  const graphs = defaultGraphs(AXES);
  assert.deepEqual(
    graphs.map((g) => g.kind),
    ["heatmap", "line"],
  );
  assert.equal(graphs[0].y, AXES[1].parameter_id);
  assert.equal(graphs[1].y, undefined);

  // One variable has no grid in it, so it gets the line on its own, full width.
  const one = defaultGraphs(AXES.slice(0, 1));
  assert.deepEqual(
    one.map((g) => [g.kind, g.wide]),
    [["line", true]],
  );
});

test("adding a graph picks a picture the screen is not already showing", () => {
  const graphs = defaultGraphs(AXES);
  const added = newGraph(AXES, graphs)!;
  const shape = (g: GraphSpec) => `${g.kind}:${g.metric}:${g.x}:${g.y ?? ""}`;
  assert.ok(!graphs.map(shape).includes(shape(added)), shape(added));
  assert.equal(needsY(added.kind) ? added.y != null : true, true);
});

test("re-running over a different set carries the layout onto it", () => {
  const graphs: GraphSpec[] = [
    {
      id: "a",
      kind: "surface",
      metric: "p50",
      x: "event:1:age",
      y: "event:3:amount",
      held: { "event:2:amount": 1 },
      wide: true,
      colour: "success",
      azimuth: 215,
      elevation: 55,
    },
  ];
  // The windfall is dropped from the sweep and the spend stays.
  const next = reconcile(graphs, [AXES[0], AXES[1]]);
  assert.equal(next[0].kind, "surface");
  assert.equal(next[0].metric, "p50");
  assert.equal(next[0].wide, true);
  assert.equal(next[0].x, "event:1:age");
  assert.equal(next[0].y, "event:2:amount");
  // How a graph is drawn and looked at is not about which variables it draws,
  // so a re-run over a different set leaves both alone.
  assert.equal(next[0].colour, "success");
  assert.equal(next[0].azimuth, 215);
  assert.equal(next[0].elevation, 55);
  // The spend was held; it is now an axis, so the hold goes rather than
  // lingering as a value nothing reads.
  assert.deepEqual(next[0].held, {});

  // Down to one variable, a surface has nothing to be a surface of.
  const alone = reconcile(graphs, [AXES[0]]);
  assert.equal(alone[0].kind, "line");
  assert.equal(alone[0].y, undefined);

  // An empty layout is seeded rather than left empty.
  assert.equal(reconcile([], AXES).length, defaultGraphs(AXES).length);
});

test("a graph is named by what it shows and what it shows it against", () => {
  const space = sweepSpace(results());
  const heat = graphView(space, {
    id: "g",
    kind: "heatmap",
    metric: "p50",
    x: "event:1:age",
    y: "event:2:amount",
    held: {},
    wide: false,
  })!;
  // Two of these three variables are amounts, so the ambiguous ones are named
  // in full and the unique one keeps its short role.
  assert.deepEqual(graphTitle(heat), {
    title: "P50 terminal net worth",
    sub: "Spending · amount × age",
  });
  assert.deepEqual(graphTitle(graphView(space, lineOver("event:1:age"))!), {
    title: "Positive ending net worth",
    sub: "by age",
  });
});

test("a role shared by two variables is not a name, so the label is used", () => {
  const names = axisNames(AXES);
  assert.equal(names.get("event:1:age"), "age");
  // "amount" belongs to both the spend and the windfall, so neither may use it.
  assert.equal(names.get("event:2:amount"), "Spending · amount");
  assert.equal(names.get("event:3:amount"), "Windfall · amount");

  // One amount in the sweep, and the short role identifies it fine.
  const alone = axisNames([AXES[0], AXES[1]]);
  assert.equal(alone.get("event:2:amount"), "amount");
});

test("the CSV carries one row per combination, in the order the server sent them", () => {
  const lines = sweepCsv(sweepSpace(results())).split("\n");
  assert.equal(lines.length, 13);
  assert.equal(
    lines[0],
    "Retire · age,Spending · amount,Windfall · amount,positive_ending_net_worth_rate,cash_funding_check_rate,p5,p50,p95",
  );
  // The last axis varies fastest, and the variables are written as values.
  assert.equal(lines[1].startsWith("60,5000,0,"), true, lines[1]);
  assert.equal(lines[2].startsWith("60,5000,100000,"), true, lines[2]);
  // Funding success was never measured, so its column is empty rather than 0.
  assert.equal(lines[1].split(",")[4], "");
});

test("the metric menu offers every measure a cell carries", () => {
  assert.deepEqual(
    METRICS.map((m) => m.id),
    ["funding", "success", "p50", "p5", "p95"],
  );
  assert.equal(metric("success").format(0.912), "91.2%");
  assert.equal(metric("p50").format(1_250_000), "$1,250,000");
  assert.equal(metric("p50").tick(1_250_000), "$1.25M");
});

test("a stored layout comes back as graphs, and junk in it does not", () => {
  const stored = [
    { id: "g7", kind: "surface", metric: "p50", colour: "success", x: "event:1:age",
      y: "event:2:amount", held: { "event:3:amount": 1 }, wide: true,
      azimuth: 62, elevation: 24 },
    // Written by a newer build, or by hand: not a kind this one can draw.
    { id: "g8", kind: "contour", metric: "success", x: "event:1:age", held: {}, wide: false },
    // A measure that no longer exists, and an axis that was never a string.
    { id: "g9", kind: "line", metric: "sharpe", x: "event:1:age", held: {}, wide: false },
    { id: "g10", kind: "line", metric: "success", x: 3, held: {}, wide: false },
    // The same card twice: the second is the one to drop.
    { id: "g7", kind: "line", metric: "success", x: "event:1:age", held: {}, wide: false },
    "not a graph at all",
  ];

  const layout = parseLayout(stored);
  assert.ok(layout);
  assert.deepEqual(
    layout.map((g) => g.id),
    ["g7"],
  );
  const [surface] = layout;
  assert.equal(surface.kind, "surface");
  assert.equal(surface.metric, "p50");
  assert.equal(surface.colour, "success");
  assert.equal(surface.y, "event:2:amount");
  assert.deepEqual(surface.held, { "event:3:amount": 1 });
  assert.equal(surface.wide, true);
  assert.equal(surface.azimuth, 62);
  assert.equal(surface.elevation, 24);

  // Nothing readable is no layout, which is what opens on the defaults.
  assert.equal(parseLayout([{ id: "g1", kind: "pie" }]), undefined);
  assert.equal(parseLayout(null), undefined);
  assert.equal(parseLayout({ graphs: [] }), undefined);

  // Ids are reserved, so a graph added after a restore is not the restored one.
  const added = newGraph(AXES, layout);
  assert.ok(added);
  assert.notEqual(added.id, "g7");
  assert.equal(
    layout.some((g) => g.id === added.id),
    false,
  );
});

test("a restored layout survives the sweep it was drawn over being re-run", () => {
  const layout = parseLayout([
    { id: "g4", kind: "heatmap", metric: "funding", x: "event:1:age",
      y: "event:2:amount", held: { "event:3:amount": 1 }, wide: false },
  ]);
  assert.ok(layout);

  // Re-run over one of its two variables and a new one: the card keeps its kind
  // and its metric, and is moved onto axes that are actually there.
  const carried = reconcile(layout, [AXES[0], AXES[2]]);
  assert.equal(carried.length, 1);
  assert.equal(carried[0].id, "g4");
  assert.equal(carried[0].kind, "heatmap");
  assert.equal(carried[0].metric, "funding");
  assert.equal(carried[0].x, "event:1:age");
  assert.equal(carried[0].y, "event:3:amount");
  // The hold named a variable that is on an axis now, so it is no longer a hold.
  assert.deepEqual(carried[0].held, {});
});


test("new graphs default to funding while saved terminal-wealth graphs keep their metric", () => {
  assert.ok(defaultGraphs(AXES).every(g => g.metric === "funding"));
  const old = lineOver("event:1:age");
  assert.equal(reconcile([old], AXES)[0].metric, "success");
  assert.equal(metric("funding").of({ success_rate: 1, funding_success_rate: null, p5: 0, p50: 0, p95: 0 }), undefined);
});
