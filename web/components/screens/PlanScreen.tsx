"use client";

import { useState } from "react";
import {
  EventInspector,
  EventsTable,
  MiniTimeline,
  NewEventDialog,
  ScenarioStrip,
} from "@/components/plan";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { UpdateScenario } from "@/lib/api/types";
import { useReorderWrite } from "@/lib/hooks/useReorderWrite";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useNav } from "@/lib/nav";
import type { PlanEvent, ScenarioParams } from "@/lib/types";
import type { PlanAxis } from "@/lib/view/axis";

/**
 * Plan tab: the scenario's parameters, its events, and the drawer inspecting
 * whichever event the table or the timeline strip has selected.
 */
export function PlanScreen({
  scenarioId,
  scenarioName,
  params,
  axis,
  events,
  raw,
  onChanged,
  onRun,
  offline,
}: {
  scenarioId: number;
  scenarioName: string;
  params: ScenarioParams;
  axis: PlanAxis;
  events: PlanEvent[];
  raw: RawWorkspace;
  onChanged: () => void;
  onRun?: () => void;
  /** Writes are being refused, so add and delete cannot be offered. */
  offline?: boolean;
}) {
  const [adding, setAdding] = useState(false);
  // The selected event is in the query, by name, so a link opens the drawer on
  // it. Derived rather than stored: an event deleted here or renamed elsewhere
  // falls back to the first row instead of leaving the drawer empty.
  const nav = useNav();
  const selected = events.find((e) => e.id === nav.selection) ?? events[0];
  const saveOrder = useReorderWrite(onChanged);

  /**
   * The strip's fields save as they are edited. `null` on the wire means
   * "leave this column alone", so a cleared birth date is a no-op the reload
   * puts back rather than an erasure.
   *
   * A refusal is rethrown rather than shown here: it belongs under the field
   * that caused it, which is what the strip does with it.
   */
  const saveParams = async (patch: Partial<ScenarioParams>) => {
    const body: UpdateScenario = {};
    if (patch.start != null) body.start_date = patch.start;
    if (patch.birthDate) body.birth_date = patch.birthDate;
    if (patch.durationYears != null) body.duration_years = patch.durationYears;
    await api.scenarios.update(scenarioId, body);
    onChanged();
  };

  const remove = async (event: PlanEvent) => {
    if (!confirm(`Delete ${event.id}?`)) return;
    try {
      await api.events.remove(scenarioId, event.serverId);
      onChanged();
    } catch (err) {
      // Refused while another event points at this one, and the message names
      // the referrers.
      alert(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <>
      <ScenarioStrip
        scenarioName={scenarioName}
        params={params}
        onChange={saveParams}
        onRun={onRun}
        offline={offline}
      />

      {events.length === 0 ? (
        <div style={{ padding: "34px 24px", maxWidth: 520 }}>
          <h4 style={{ margin: "0 0 6px" }}>No events</h4>
          <p
            style={{
              margin: "0 0 14px",
              fontSize: 13,
              lineHeight: 1.5,
              color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
            }}
          >
            Events are what makes the plan move: income arriving, spending leaving, a
            retirement age pausing one and starting another. Without any, the
            simulation just compounds the opening balances.
          </p>
          <Button
            variant="primary"
            shortcut="a"
            onClick={() => setAdding(true)}
            disabled={offline}
          >
            Add event
          </Button>
        </div>
      ) : (
        /* Not SplitPane: the timeline strip spans the full height of the list
           column and sits flush against its bottom edge. */
        <div style={{ display: "grid", gridTemplateColumns: "1fr 344px" }}>
          <div
            style={{
              borderRight: "1px solid var(--color-divider)",
              display: "flex",
              flexDirection: "column",
              minWidth: 0,
            }}
          >
            <div style={{ padding: "16px 20px", flex: 1 }}>
              <div
                style={{
                  display: "flex",
                  alignItems: "baseline",
                  justifyContent: "space-between",
                  marginBottom: 8,
                }}
              >
                <h4 style={{ margin: 0 }}>
                  Events{" "}
                  <span className="text-muted" style={{ fontSize: 13 }}>
                    {events.length}
                  </span>
                </h4>
                <Button
                  shortcut="a"
                  onClick={() => setAdding(true)}
                  disabled={offline}
                  title={offline ? "No connection to the server." : undefined}
                >
                  Add event
                </Button>
              </div>

              <EventsTable
                events={events}
                selectedId={selected?.id ?? ""}
                onSelect={nav.setSelection}
                onReorder={
                  offline
                    ? undefined
                    : (ids) => saveOrder(() => api.events.reorder(scenarioId, ids))
                }
              />
            </div>

            <MiniTimeline
              events={events}
              axis={axis}
              selectedId={selected?.id ?? ""}
              onSelect={nav.setSelection}
            />
          </div>

          <aside>
            {selected && (
              <EventInspector
                event={selected}
                onDelete={offline ? undefined : () => remove(selected)}
              />
            )}
          </aside>
        </div>
      )}

      {adding && (
        <NewEventDialog
          scenarioId={scenarioId}
          accounts={raw.accounts}
          assets={raw.assets}
          events={raw.events}
          onClose={() => setAdding(false)}
          onCreated={onChanged}
        />
      )}
    </>
  );
}
