"use client";

import { useState } from "react";
import {
  EventInspector,
  EventsTable,
  MiniTimeline,
  ScenarioStrip,
} from "@/components/plan";
import { Button } from "@/components/ui";
import {
  MOCK_EVENTS,
  MOCK_SCENARIO_PARAMS,
  PLAN_AGE_RANGE,
} from "@/lib/mock/events";
import type { EventId } from "@/lib/types";

/**
 * Artboard 3b — 2a's Plan screen (scenario strip, event list, event drawer)
 * with the mini-timeline pinned to the footer of the list column. The table
 * and the strip drive the same selection, so the drawer follows either.
 */
export function PlanScreen({
  scenarioName,
  onRun,
}: {
  scenarioName: string;
  onRun?: () => void;
}) {
  const [params, setParams] = useState(MOCK_SCENARIO_PARAMS);
  const [events] = useState(MOCK_EVENTS);
  const [selectedId, setSelectedId] = useState<EventId>(events[4].id);

  const selected = events.find((e) => e.id === selectedId) ?? events[0];

  return (
    <>
      <ScenarioStrip
        scenarioName={scenarioName}
        params={params}
        onChange={setParams}
        onRun={onRun}
      />

      {/* Not SplitPane: the timeline strip has to span the full height of the
          list column and sit flush against its bottom edge, so this column is
          a flex container rather than a padded block. */}
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
              <Button shortcut="a">Add event</Button>
            </div>

            <EventsTable
              events={events}
              selectedId={selectedId}
              onSelect={setSelectedId}
            />

            <p
              style={{
                fontSize: 12,
                margin: "12px 0 0",
                color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
              }}
            >
              One screen instead of two: the Scenario tab was six fields and a bracket
              table, never a destination of its own. The bracket table moves behind{" "}
              <em>Tax config…</em> in the drawer&rsquo;s footer.
            </p>
          </div>

          <MiniTimeline
            events={events}
            ageRange={PLAN_AGE_RANGE}
            selectedId={selectedId}
            onSelect={setSelectedId}
          />
        </div>

        <aside>
          <EventInspector event={selected} />
        </aside>
      </div>
    </>
  );
}
