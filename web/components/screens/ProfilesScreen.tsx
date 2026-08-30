"use client";

import { useState } from "react";
import { SplitPane } from "@/components/layout";
import {
  InflationProfilesTable,
  ProfileInspector,
  ReturnProfilesTable,
} from "@/components/profiles";
import { Button } from "@/components/ui";
import {
  MOCK_INFLATION_PROFILES,
  MOCK_RETURN_PROFILES,
  isReadOnlyPreset,
} from "@/lib/mock/profiles";
import type { ReturnProfileId } from "@/lib/types";

/**
 * Artboard 3c — Portfolio › Profiles. Return behaviour is data, not code:
 * every asset points at one of these.
 */
export function ProfilesScreen() {
  const [profiles] = useState(MOCK_RETURN_PROFILES);
  const [selectedId, setSelectedId] = useState<ReturnProfileId>(profiles[0].id);
  const [activeInflationId, setActiveInflationId] = useState(
    MOCK_INFLATION_PROFILES[0].id,
  );

  const selected = profiles.find((p) => p.id === selectedId) ?? profiles[0];

  return (
    <SplitPane
      railWidth={344}
      main={
        <div style={{ padding: "16px 20px" }}>
          <div
            style={{
              display: "flex",
              alignItems: "baseline",
              justifyContent: "space-between",
              marginBottom: 8,
            }}
          >
            <h4 style={{ margin: 0 }}>Return profiles</h4>
            <Button shortcut="a">New profile</Button>
          </div>

          <ReturnProfilesTable
            profiles={profiles}
            selectedId={selectedId}
            onSelect={setSelectedId}
          />

          <h4 style={{ margin: "24px 0 8px" }}>Inflation profiles</h4>
          <InflationProfilesTable
            profiles={MOCK_INFLATION_PROFILES}
            activeId={activeInflationId}
            onActivate={setActiveInflationId}
          />

          <p
            style={{
              fontSize: 12,
              margin: "12px 0 0",
              color: "color-mix(in srgb, var(--color-text) 55%, transparent)",
            }}
          >
            One inflation profile is active per scenario; the others stay as variants
            for the What-if panel on Results. Historical presets are read-only —
            duplicate to edit.
          </p>
        </div>
      }
      rail={
        <ProfileInspector profile={selected} readOnly={isReadOnlyPreset(selected)} />
      }
    />
  );
}
