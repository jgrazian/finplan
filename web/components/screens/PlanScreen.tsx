"use client";

import { useState } from "react";
import {
  type EventDraft,
  EventEditor,
  EventRail,
  PlanTimeline,
  ScenarioStrip,
  eventProblem,
  toEventBody,
} from "@/components/plan";
import { Button } from "@/components/ui";
import { api } from "@/lib/api/client";
import type { Event as ApiEvent, EventBody, UpdateScenario } from "@/lib/api/types";
import { useReorderWrite } from "@/lib/hooks/useReorderWrite";
import { useSubmit } from "@/lib/hooks/useSubmit";
import type { RawWorkspace } from "@/lib/hooks/useWorkspace";
import { useNav } from "@/lib/nav";
import type { AssumptionChoices, PlanEvent, ScenarioParams } from "@/lib/types";
import type { PlanAxis } from "@/lib/view/axis";
import { ParameterEditor, ParameterRail, blankParameterValue } from "@/components/plan/Parameters";

/**
 * What Add event makes: a yearly repeat that does nothing yet.
 *
 * The old dialog asked for a name, a trigger and an effect before it would
 * create anything, which is three decisions to make about an event you have
 * not started thinking about. The editor asks the same questions better, and
 * with the row already on the rail there is something to abandon rather than a
 * form to cancel. Yearly because it is the interval that reads as a placeholder
 * — nobody means "every year" and forgets to change it, the way they might with
 * monthly.
 */
function blankEvent(name: string): EventBody {
  return {
    name,
    description: null,
    fires_once: false,
    enabled: true,
    // Omitted, so it lands at the end of the rail rather than on top of it.
    sort_order: null,
    trigger: {
      kind: "Repeating",
      interval: "Yearly",
      start_condition: null,
      end_condition: null,
      max_occurrences: null,
    },
    effects: [],
  };
}

/**
 * Plan tab, in the shape of artboard 10a: the scenario as a line across the
 * top, the events as a rail on the left, the editor for the selected one
 * filling the width beside it, and the whole plan as a band of lanes along the
 * bottom.
 *
 * Two columns rather than three, so the editor is wide enough to put when an
 * event fires beside what it does instead of stacking them down a 400px rail.
 */
export function PlanScreen({
  scenarioId,
  params,
  assumptions,
  axis,
  events,
  raw,
  onChanged,
  offline,
}: {
  scenarioId: number;
  params: ScenarioParams;
  assumptions: AssumptionChoices;
  axis: PlanAxis;
  events: PlanEvent[];
  raw: RawWorkspace;
  onChanged: () => void;
  /** Writes are being refused, so add and delete cannot be offered. */
  offline?: boolean;
}) {
  const [savedAt, setSavedAt] = useState<Map<number, number>>(new Map());
  const [lastEventId, setLastEventId] = useState<string>();
  const editing = useSubmit();
  // The selected event is in the query, by name, so a link opens the editor on
  // it. Derived rather than stored: an event deleted here or renamed elsewhere
  // falls back to the first row instead of leaving the editor empty.
  const nav = useNav();
  const parameterId = nav.selection?.startsWith("parameter:")
    ? Number(nav.selection.slice("parameter:".length)) : undefined;
  const parameters = raw.parameters;
  const selectedParameter = parameters.find((p) => p.id === parameterId);
  const selected = events.find((e) => e.id === (parameterId == null ? nav.selection : lastEventId)) ?? events[0];
  const selectedRaw = raw.events.find((e) => e.id === selected?.serverId);
  const selectEvent = (name: string) => { setLastEventId(name); nav.setSelection(name); };
  const selectParameter = (id: number) => {
    if (parameterId == null && selected) setLastEventId(selected.id);
    nav.setSelection(`parameter:${id}`);
  };
  const saveOrder = useReorderWrite(onChanged);
  // The pickers' choices, plus the two things that let a condition explain
  // itself: the plan's birth date, and which event is being edited.
  const context = {
    accounts: raw.accounts,
    assets: raw.assets,
    events: raw.events,
    parameters,
    scenarioId,
    birthDate: params.birthDate || undefined,
    selfId: selectedRaw?.id,
  };

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
    if (patch.name != null) {
      const name = patch.name.trim();
      if (!name) throw new Error("Enter a scenario name.");
      body.name = name;
    }
    if (patch.start != null) body.start_date = patch.start;
    if (patch.birthDate) body.birth_date = patch.birthDate;
    if (patch.durationYears != null) body.duration_years = patch.durationYears;
    if (patch.inflationProfileId != null) {
      body.inflation_profile_id = patch.inflationProfileId;
    }
    if (patch.taxConfigId != null) body.tax_config_id = patch.taxConfigId;
    await api.scenarios.update(scenarioId, body);
    onChanged();
  };

  /**
   * Saves the editor's whole event. PUT, not PATCH — a trigger is a tree, and
   * there is no partial merge into one that means anything, so what the editor
   * holds is what the event becomes.
   */
  const apply = (event: PlanEvent, draft: EventDraft) => {
    const problem = eventProblem(draft);
    if (problem) return editing.fail(problem);
    const name = draft.name.trim();
    editing.run(
      () => api.events.replace(scenarioId, event.serverId, toEventBody(draft)),
      () => {
        setSavedAt((m) => new Map(m).set(event.serverId, Date.now()));
        // The query holds the selection by name, so a rename has to carry it —
        // otherwise saving one drops the editor back onto the first row.
        if (name !== event.id) selectEvent(name);
        onChanged();
      },
    );
  };

  /**
   * Adds an event and opens it. No dialog: an empty event is a valid one, and
   * the editor beside the rail is a better place to answer what it does than a
   * modal that asks the same things in a narrower column.
   */
  const add = () => {
    const name = freeName("New Event", raw.events);
    editing.run(
      () => api.events.create(scenarioId, blankEvent(name)),
      () => {
        selectEvent(name);
        onChanged();
      },
    );
  };

  /**
   * Copies the saved event, under a name no other event has, and opens the
   * copy. Offered only when the editor is clean, so "duplicate" cannot quietly
   * mean two different things depending on what is typed but not applied.
   */
  const duplicate = (source: ApiEvent) => {
    const name = freeName(`${source.name} copy`, raw.events);
    editing.run(
      () =>
        api.events.create(scenarioId, {
          name,
          description: source.description,
          fires_once: source.fires_once,
          enabled: source.enabled,
          // Omitted, so the copy lands at the end rather than on top of the
          // event it was made from.
          sort_order: null,
          trigger: source.trigger,
          effects: source.effects,
        }),
      () => {
        selectEvent(name);
        onChanged();
      },
    );
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

  const addParameter = (kind: ReturnType<typeof blankParameterValue>["kind"]) => {
    const names = new Set(parameters.map((p) => p.name));
    let name = `New ${kind}`;
    for (let suffix = 2; names.has(name); suffix += 1) name = `New ${kind} ${suffix}`;
    let createdId: number | undefined;
    editing.run(
      async () => { createdId = (await api.parameters.create(scenarioId, { name, value: blankParameterValue(kind) })).id; },
      () => {
        if (createdId != null) selectParameter(createdId);
        onChanged();
      },
    );
  };

  return (
    <>
      <ScenarioStrip
        params={params}
        assumptions={assumptions}
        onChange={saveParams}
        offline={offline}
      />

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "300px 1fr",
          alignItems: "stretch",
          minHeight: 460,
        }}
      >
        <div style={{ borderRight: "1px solid var(--color-divider)", minWidth: 0, display: "grid", gridTemplateRows: "minmax(0, 1fr) auto", height: "min(75vh, 740px)", minHeight: 430, alignSelf: "start", overflow: "hidden" }}>
          <div style={{ minHeight: 0, overflow: "auto" }}>
            <EventRail
              events={events}
              selectedId={parameterId == null ? selected?.id ?? "" : ""}
              onSelect={selectEvent}
              onAdd={add}
              adding={editing.busy}
              onReorder={offline ? undefined : (ids) => saveOrder(() => api.events.reorder(scenarioId, ids))}
              offline={offline}
            />
          </div>
          <ParameterRail parameters={parameters} selectedId={parameterId}
            onSelect={selectParameter} onAdd={addParameter} adding={editing.busy}
            submitError={editing.error} offline={offline} />
        </div>
        <div style={{ minWidth: 0 }}>
          {selectedParameter && <ParameterEditor key={`${scenarioId}:${selectedParameter.id}`}
            parameter={selectedParameter} scenarioId={scenarioId}
            onSaved={onChanged}
            onDeleted={() => { nav.setSelection(selected?.id); onChanged(); }}
            onSelectEvent={selectEvent} offline={offline} />}
          {!selectedParameter && events.length === 0 && <div style={{ padding: "34px 24px", maxWidth: 520 }}>
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
            onClick={add}
            disabled={offline || editing.busy}
          >
            Add event
          </Button>
          {editing.error && (
            <p style={{ margin: "10px 0 0", fontSize: 12, color: "var(--color-accent-900)" }}>
              {editing.error}
            </p>
          )}
          </div>}
          {selected && selectedRaw && (
            <div style={{ display: selectedParameter ? "none" : undefined }}>
              <EventEditor
                // A fresh draft per row: edits are not carried across events.
                key={selectedRaw.id}
                event={selected}
                raw={selectedRaw}
                context={context}
                onApply={(draft) => apply(selected, draft)}
                onDuplicate={offline ? undefined : () => duplicate(selectedRaw)}
                onDelete={offline ? undefined : () => remove(selected)}
                onSelectEvent={selectEvent}
                busy={editing.busy}
                error={editing.error}
                savedAt={savedAt.get(selected.serverId)}
                offline={offline}
              />
            </div>
          )}
        </div>
      </div>
      {events.length > 0 && <PlanTimeline
        events={events}
        axis={axis}
        selectedId={parameterId == null ? selected?.id ?? "" : ""}
        onSelect={selectEvent}
      />}

    </>
  );
}

/** `New Event`, then `New Event 2`: the server enforces unique names, so an
 *  event has to arrive with one rather than be refused for a name nobody
 *  chose. */
function freeName(base: string, events: ApiEvent[]): string {
  const taken = new Set(events.map((e) => e.name));
  if (!taken.has(base)) return base;
  for (let n = 2; ; n += 1) {
    if (!taken.has(`${base} ${n}`)) return `${base} ${n}`;
  }
}
