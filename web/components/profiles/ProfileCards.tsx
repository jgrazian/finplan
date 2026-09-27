"use client";

import { CLASS_LABEL } from "@/lib/tickers";
import type { InflationProfile, ReturnProfile } from "@/lib/types";
import { KIND_LABEL } from "./DistributionTerms";
import { bandLabel, pct } from "./distribution";

/**
 * The return profile library on a phone: name and asset class on top, kind,
 * 5th…95th band and what uses it underneath, the mean against the right
 * margin. The shape sparkline needs the table's shared axis to mean anything,
 * so it stays with the table.
 */
export function ProfileCards({
  profiles,
  selectedId,
  onSelect,
}: {
  profiles: ReturnProfile[];
  selectedId: string | undefined;
  onSelect: (profile: ReturnProfile) => void;
}) {
  return (
    <ul className="row-cards" aria-label="Return profile library">
      {profiles.map((profile) => (
        <li key={profile.id}>
          <button
            type="button"
            className="row-card"
            aria-current={profile.id === selectedId || undefined}
            onClick={() => onSelect(profile)}
          >
            <span className="row-card-text">
              <span className="row-card-title">
                <span className="row-card-name-strong">{profile.id}</span>
                {profile.assetClass && (
                  <span className="row-card-name">{CLASS_LABEL[profile.assetClass]}</span>
                )}
              </span>
              <span className="row-card-meta">
                {[
                  KIND_LABEL[profile.kind],
                  bandLabel(profile.distribution, profile.history),
                  profile.usedBy.length > 0 ? `used by ${profile.usedBy.join(", ")}` : null,
                ]
                  .filter(Boolean)
                  .join(" · ")}
              </span>
            </span>
            <span className="row-card-figure">{pct(profile.mean)}</span>
            <span className="row-card-chevron" aria-hidden>
              ›
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * Inflation on a phone. There is no inspector to open, so the card is the
 * radio: tapping one makes it the scenario's profile, as the radio in the
 * table's last column does.
 */
export function InflationCards({
  profiles,
  activeId,
  onActivate,
}: {
  profiles: InflationProfile[];
  activeId: string;
  onActivate?: (profile: InflationProfile) => void;
}) {
  return (
    <ul className="row-cards" aria-label="Inflation profiles">
      {profiles.map((profile) => (
        <li key={profile.id}>
          <label className="row-card" data-disabled={!onActivate || undefined}>
            <span className="row-card-text">
              <span className="row-card-name-strong">{profile.id}</span>
              <span className="row-card-meta">
                {KIND_LABEL[profile.kind]} · {bandLabel(profile.distribution)}
              </span>
            </span>
            <span className="row-card-figure">{pct(profile.mean)}</span>
            <span className="check">
              <input
                type="radio"
                name="inflation-profile-card"
                checked={profile.id === activeId}
                disabled={!onActivate}
                onChange={() => onActivate?.(profile)}
                aria-label={`Use ${profile.id}`}
              />
              <span className="box" />
            </span>
          </label>
        </li>
      ))}
    </ul>
  );
}
