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
import { Button, Dialog, SegmentedControl } from "@/components/ui";
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
 * Plan tab, in the shape of artboard 17a: the scenario as a line across the
 * top, a rail on the left that switches between the events and the parameters
 * they use, the editor for the selected row filling the width beside it, and
 * the whole plan as a band of lanes along the bottom.
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
  // The rail shows one list at a time. It follows the selection, except when
  // Parameters was picked with none to select — then it holds on the empty list.
  const [emptyParameters, setEmptyParameters] = useState(false);
  const railMode: RailMode =
    parameterId != null || (emptyParameters && parameters.length === 0) ? "parameters" : "events";
  const selectEvent = (name: string) => {
    setEmptyParameters(false);
    setLastEventId(name);
    nav.setSelection(name);
  };
  const selectParameter = (id: number) => {
    if (parameterId == null && selected) setLastEventId(selected.id);
    nav.setSelection(`parameter:${id}`);
  };
  const switchRail = (mode: RailMode) => {
    if (mode === railMode) return;
    if (mode === "events") {
      setEmptyParameters(false);
      nav.setSelection(lastEventId ?? events[0]?.id);
    } else if (parameters[0]) {
      selectParameter(parameters[0].id);
    } else {
      if (selected) setLastEventId(selected.id);
      setEmptyParameters(true);
    }
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

  // Asked in the app's own dialog rather than the browser's. A refusal — the
  // server will not delete an event another one points at, and names the
  // referrers — lands inside it, beside the question it answers.
  const [deleting, setDeleting] = useState<PlanEvent>();
  const [removing, setRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<string>();
  const askRemove = (event: PlanEvent) => {
    setRemoveError(undefined);
    setDeleting(event);
  };
  const remove = async (event: PlanEvent) => {
    setRemoving(true);
    setRemoveError(undefined);
    try {
      await api.events.remove(scenarioId, event.serverId);
      setDeleting(undefined);
      onChanged();
    } catch (err) {
      setRemoveError(err instanceof Error ? err.message : String(err));
    } finally {
      setRemoving(false);
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
        <div style={{ borderRight: "1px solid var(--color-divider)", minWidth: 0, display: "grid", gridTemplateRows: "auto minmax(0, 1fr)", height: "min(78vh, 780px)", minHeight: 430, alignSelf: "start", overflow: "hidden" }}>
          {/* Artboard 17a — one rail, two lists, at the same weight. */}
          <div style={{ padding: "14px 18px 12px", display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
            <SegmentedControl<RailMode>
              ariaLabel="Rail"
              value={railMode}
              options={[
                { value: "events", label: `Events ${events.length}` },
                { value: "parameters", label: `Parameters ${parameters.length}` },
              ]}
              onChange={switchRail}
            />
            <Button
              variant="ghost"
              shortcut={railMode === "events" ? "a" : undefined}
              onClick={railMode === "events" ? add : () => addParameter("Money")}
              disabled={offline || editing.busy}
              aria-label={railMode === "events" ? "Add event" : "Add parameter"}
              title={offline ? "No connection to the server." : undefined}
            >
              Add
            </Button>
          </div>
          <div style={{ minHeight: 0, overflow: "auto" }}>
            {railMode === "events" ? (
              <EventRail
                events={events}
                selectedId={selected?.id ?? ""}
                onSelect={selectEvent}
                onReorder={offline ? undefined : (ids) => saveOrder(() => api.events.reorder(scenarioId, ids))}
              />
            ) : (
              <ParameterRail parameters={parameters} selectedId={parameterId}
                onSelect={selectParameter} error={editing.error} />
            )}
          </div>
        </div>
        <div style={{ minWidth: 0 }}>
          {selectedParameter && <ParameterEditor key={`${scenarioId}:${selectedParameter.id}`}
            parameter={selectedParameter} scenarioId={scenarioId}
            onSaved={onChanged}
            onDeleted={() => {
              const next = parameters.find((p) => p.id !== selectedParameter.id);
              if (next) selectParameter(next.id);
              else { setEmptyParameters(true); nav.setSelection(lastEventId ?? selected?.id); }
              onChanged();
            }}
            onSelectEvent={selectEvent} offline={offline} />}
          {railMode === "parameters" && !selectedParameter && (
            <Empty
              title="No parameters"
              body="A parameter is a named value — a retirement age, a spending level — that amounts and triggers refer to by name, and that Analysis offers as an axis to sweep or solve for. Change it once and every event using it follows."
              action="Add parameter"
              onAction={() => addParameter("Money")}
              disabled={offline || editing.busy}
              error={editing.error}
            />
          )}
          {railMode === "events" && events.length === 0 && (
            <Empty
              title="No events"
              body="Events are what makes the plan move: income arriving, spending leaving, a retirement age pausing one and starting another. Without any, the simulation just compounds the opening balances."
              action="Add event"
              shortcut="a"
              onAction={add}
              disabled={offline || editing.busy}
              error={editing.error}
            />
          )}
          {selected && selectedRaw && (
            // Kept mounted under a parameter, so an unsaved draft survives a
            // look at the parameter it uses.
            <div style={{ display: railMode === "parameters" ? "none" : undefined }}>
              <EventEditor
                // A fresh draft per row: edits are not carried across events.
                key={selectedRaw.id}
                event={selected}
                raw={selectedRaw}
                context={context}
                onApply={(draft) => apply(selected, draft)}
                onDuplicate={offline ? undefined : () => duplicate(selectedRaw)}
                onDelete={offline ? undefined : () => askRemove(selected)}
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
        selectedId={railMode === "events" ? selected?.id ?? "" : ""}
        onSelect={selectEvent}
      />}

      {deleting && (
        <Dialog
          title="Delete event"
          submitLabel="Delete"
          busy={removing}
          error={removeError}
          onClose={() => setDeleting(undefined)}
          onSubmit={() => remove(deleting)}
        >
          <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
            Delete <span className="cd-name">{deleting.id}</span>
            {deleting.effects.length > 0
              ? ` and its ${deleting.effects.length === 1 ? "effect" : `${deleting.effects.length} effects`}`
              : ""}
            ? The next run goes without it. To keep its setup but leave it out of runs, untick
            Enabled instead.
          </p>
        </Dialog>
      )}
    </>
  );
}

type RailMode = "events" | "parameters";

/** What the editor pane says when the rail's list is empty. */
function Empty({ title, body, action, shortcut, onAction, disabled, error }: {
  title: string;
  body: string;
  action: string;
  shortcut?: string;
  onAction: () => void;
  disabled?: boolean;
  error?: string;
}) {
  return (
    <div style={{ padding: "34px 28px", maxWidth: 520 }}>
      <h4 style={{ margin: "0 0 6px" }}>{title}</h4>
      <p
        style={{
          margin: "0 0 14px",
          fontSize: 13,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        {body}
      </p>
      <Button variant="primary" shortcut={shortcut} onClick={onAction} disabled={disabled}>
        {action}
      </Button>
      {error && (
        <p style={{ margin: "10px 0 0", fontSize: 12, color: "var(--color-accent-900)" }}>{error}</p>
      )}
    </div>
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
