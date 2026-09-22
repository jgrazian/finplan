"use client";

import { Button, Dropdown, Field, RangeField, SegmentedControl, StatLabel } from "@/components/ui";
import type { SegmentOption } from "@/components/ui";
import { paramTick } from "@/lib/view/analysis";
import {
  MAX_ELEVATION,
  METRICS,
  MIN_ELEVATION,
  axisNames,
  clampElevation,
  graphTitle,
  needsY,
  type GraphKind,
  type GraphSpec,
  type GraphView,
  type MetricId,
  type SweepSpace,
} from "@/lib/view/sweep";

const KINDS: ReadonlyArray<SegmentOption<GraphKind>> = [
  { value: "line", label: "Line" },
  { value: "heatmap", label: "Heatmap" },
  { value: "surface", label: "Surface" },
];

/**
 * Everything about the selected graph, in one column.
 *
 * One inspector rather than a set of controls on every card: the choices are
 * the same for all of them, and putting them in one place is what lets four
 * graphs of the same sweep fit on a screen and still read as pictures.
 */
export function GraphInspector({
  space,
  view,
  position,
  count,
  onChange,
  onRemove,
  onReset,
  onSolveFor,
}: {
  space: SweepSpace;
  view: GraphView;
  position: number;
  count: number;
  onChange: (next: GraphSpec) => void;
  onRemove: () => void;
  onReset: () => void;
  /** Hand a variable to Solve, which is where a steep graph usually points. */
  onSolveFor: (parameterId: string) => void;
}) {
  const spec = view.spec;
  const { title, sub } = graphTitle(view);

  const names = axisNames(space.axes);
  const axisOptions = (exclude: string | undefined) =>
    space.axes.map((axis) => ({
      value: axis.parameter_id,
      label: names.get(axis.parameter_id) ?? axis.role,
      detail: axis.label,
      disabled: axis.parameter_id === exclude,
    }));

  const setKind = (kind: GraphKind) => {
    if (!needsY(kind)) {
      onChange({ ...spec, kind });
      return;
    }
    // A second axis is required from here on, and the first free variable is
    // the least surprising one to hand it: switching to a heatmap should draw
    // a heatmap, not ask a question first.
    const y =
      spec.y && spec.y !== spec.x
        ? spec.y
        : space.axes.find((axis) => axis.parameter_id !== spec.x)?.parameter_id;
    if (y == null) return;
    onChange({ ...spec, kind, y });
  };

  const twoDimensional = needsY(spec.kind);
  const canHoldTwo = space.axes.length > 1;

  return (
    <div
      style={{
        padding: "14px 16px 18px",
        display: "flex",
        flexDirection: "column",
        gap: 14,
        height: "100%",
      }}
    >
      <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
        <h6 style={{ margin: 0 }}>Graph {position}</h6>
        <span
          style={{
            fontSize: 11.5,
            color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          }}
        >
          of {count}
        </span>
        <button type="button" className="sbtn" style={{ marginLeft: "auto" }} onClick={onRemove}>
          Remove
        </button>
      </div>

      <Field label="Kind">
        <SegmentedControl
          ariaLabel="Graph kind"
          options={
            canHoldTwo ? KINDS : KINDS.map((k) => ({ ...k, disabled: k.value !== "line" }))
          }
          value={spec.kind}
          onChange={setKind}
        />
      </Field>

      <Field label="Shows">
        <Dropdown
          ariaLabel="Metric"
          options={METRICS.map((m) => ({ value: m.id, label: m.label }))}
          value={spec.metric}
          onChange={(metric) => onChange({ ...spec, metric: metric as MetricId })}
        />
      </Field>

      <Field label="X axis">
        <Dropdown
          ariaLabel="Horizontal axis"
          options={axisOptions(twoDimensional ? spec.y : undefined)}
          value={spec.x}
          onChange={(x) => onChange({ ...spec, x })}
        />
      </Field>

      {twoDimensional && spec.y != null && (
        <Field label="Y axis">
          <Dropdown
            ariaLabel="Vertical axis"
            options={axisOptions(spec.x)}
            value={spec.y}
            onChange={(y) => onChange({ ...spec, y })}
          />
        </Field>
      )}

      {/* A surface's height already carries the metric above, so its colour is
          a channel going spare. Offered only here: a heatmap has nothing but
          colour, and a line has no colour at all. */}
      {spec.kind === "surface" && (
        <Field label="Colour">
          <Dropdown
            ariaLabel="Colour metric"
            options={[
              { value: "", label: "same as the height", detail: view.metric.short },
              ...METRICS.filter((m) => m.id !== spec.metric).map((m) => ({
                value: m.id,
                label: m.label,
              })),
            ]}
            value={spec.colour != null && spec.colour !== spec.metric ? spec.colour : ""}
            onChange={(colour) =>
              onChange({ ...spec, colour: colour === "" ? undefined : (colour as MetricId) })
            }
          />
        </Field>
      )}

      {spec.kind === "surface" && (
        <div>
          <div style={{ display: "flex", alignItems: "baseline", gap: 8 }}>
            <StatLabel>view</StatLabel>
            <button
              type="button"
              className="sbtn"
              style={{ marginLeft: "auto" }}
              onClick={() =>
                onChange({ ...spec, azimuth: undefined, elevation: undefined })
              }
            >
              Reset
            </button>
          </div>
          <RangeField
            label={`Turn ${Math.round(view.azimuth)}°`}
            value={view.azimuth}
            min={0}
            max={360}
            step={5}
            onValueChange={(azimuth) => onChange({ ...spec, azimuth })}
          />
          <RangeField
            label={`Tilt ${Math.round(view.elevation)}°`}
            value={view.elevation}
            min={MIN_ELEVATION}
            max={MAX_ELEVATION}
            step={2}
            onValueChange={(elevation) =>
              onChange({ ...spec, elevation: clampElevation(elevation) })
            }
          />
          <p
            style={{
              fontSize: 11,
              margin: "2px 0 0",
              color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
              textWrap: "pretty",
            }}
          >
            Drag the surface to turn it. Which matters because a surface hides
            one corner behind its own ridge, and turning it is how you see what
            the ridge is in front of.
          </p>
        </div>
      )}

      {view.held.length > 0 && (
        <div>
          <StatLabel>held</StatLabel>
          <div style={{ display: "flex", flexDirection: "column", gap: 6, marginTop: 6 }}>
            {view.held.map((held) => (
              <div
                key={held.axis.parameter_id}
                style={{ display: "flex", alignItems: "center", gap: 6 }}
              >
                <span
                  style={{
                    fontFamily: "ui-monospace, Menlo, monospace",
                    fontSize: 11,
                    flex: 1,
                    minWidth: 0,
                    overflow: "hidden",
                    textOverflow: "ellipsis",
                    whiteSpace: "nowrap",
                  }}
                  title={held.axis.label}
                >
                  {held.name}
                </span>
                <Dropdown
                  inline
                  ariaLabel={`${held.name} held at`}
                  options={held.axis.values.map((value, index) => ({
                    value: String(index),
                    label: paramTick(held.axis.kind, value),
                    detail: index === space.planIndices?.[space.slotOf(held.axis.parameter_id)]
                      ? "the plan"
                      : undefined,
                  }))}
                  value={String(held.index)}
                  onChange={(index) =>
                    onChange({
                      ...spec,
                      held: { ...spec.held, [held.axis.parameter_id]: Number(index) },
                    })
                  }
                />
              </div>
            ))}
          </div>
          <p
            style={{
              fontSize: 11,
              margin: "6px 0 0",
              color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
              textWrap: "pretty",
            }}
          >
            Variables not on an axis sit at the plan&apos;s own value until you
            slice them somewhere else.
          </p>
        </div>
      )}

      <Field label="Width">
        <Button block onClick={() => onChange({ ...spec, wide: !spec.wide })}>
          {spec.wide ? "Full column → halve" : "Half column → widen"}
        </Button>
      </Field>

      <Button
        block
        variant="secondary"
        title={`Goal seek ${view.yAxis?.label ?? view.xAxis.label}`}
        onClick={() => onSolveFor(view.yAxis?.parameter_id ?? view.xAxis.parameter_id)}
      >
        Solve {view.yName ?? view.xName} exactly
      </Button>

      <p
        style={{
          fontSize: 11.5,
          margin: "auto 0 0",
          color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
          textWrap: "pretty",
        }}
      >
        {title} {sub}
        {view.twoMeasures && `, coloured by ${view.colour.short}`}. Reads the
        finished sweep — no re-run.
      </p>

      <button type="button" className="sbtn" style={{ alignSelf: "flex-start" }} onClick={onReset}>
        Reset layout
      </button>
    </div>
  );
}
