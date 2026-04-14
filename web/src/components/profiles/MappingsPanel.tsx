"use client";

import { useMemo } from "react";
import type { Account, AssetMapping, ReturnProfile } from "@/lib/types";
import * as api from "@/lib/api";
import { getHistoricalPresetForTicker, getSuggestion } from "@/lib/tickerSuggestions";

interface MappingsPanelProps {
  accounts: Account[];
  profiles: ReturnProfile[];
  mappings: AssetMapping[];
  onChanged: () => void;
}

interface AssetRow {
  name: string;
  mapping?: AssetMapping;
  suggestedProfileId?: number;
  suggestedProfileName?: string;
}

export function MappingsPanel({
  accounts,
  profiles,
  mappings,
  onChanged,
}: MappingsPanelProps) {
  const uniqueAssets = useMemo(() => {
    const assets = new Set<string>();
    for (const acct of accounts) {
      if (acct.category !== "Investment" || !acct.holdings) continue;
      for (const h of acct.holdings) {
        if (h.asset_name) assets.add(h.asset_name);
      }
    }
    return Array.from(assets).sort();
  }, [accounts]);

  const mappingByAsset = useMemo(() => {
    const map = new Map<string, AssetMapping>();
    for (const m of mappings) map.set(m.asset_name, m);
    return map;
  }, [mappings]);

  const profileByName = useMemo(() => {
    const map = new Map<string, ReturnProfile>();
    for (const p of profiles) map.set(p.name, p);
    return map;
  }, [profiles]);

  // Index bootstrap profiles by their historical preset so a preset-based
  // ticker suggestion can match against Bootstrap profiles too.
  const bootstrapByPreset = useMemo(() => {
    const map = new Map<string, ReturnProfile>();
    for (const p of profiles) {
      if (p.profile_type === "Bootstrap" && p.preset) {
        // Prefer i.i.d. profiles for the suggestion if multiple exist.
        if (!map.has(p.preset) || !p.block_size) {
          map.set(p.preset, p);
        }
      }
    }
    return map;
  }, [profiles]);

  const rows: AssetRow[] = useMemo(() => {
    return uniqueAssets.map((name) => {
      const suggestion = getSuggestion(name);
      let suggestedProfile: ReturnProfile | undefined;
      if (suggestion && profileByName.has(suggestion.profile_name)) {
        suggestedProfile = profileByName.get(suggestion.profile_name);
      } else {
        const preset = getHistoricalPresetForTicker(name);
        if (preset && bootstrapByPreset.has(preset)) {
          suggestedProfile = bootstrapByPreset.get(preset);
        }
      }
      return {
        name,
        mapping: mappingByAsset.get(name),
        suggestedProfileId: suggestedProfile?.id,
        suggestedProfileName: suggestedProfile?.name,
      };
    });
  }, [uniqueAssets, mappingByAsset, profileByName, bootstrapByPreset]);

  // Account-level return profile mappings (non-brokerage investment accounts).
  const mappableAccounts = useMemo(
    () =>
      accounts.filter(
        (a) =>
          a.category === "Investment" &&
          a.account_type !== "Brokerage"
      ),
    [accounts]
  );

  async function setAssetMapping(assetName: string, profileId: number | null) {
    if (profileId === null) {
      await api.deleteMapping(assetName);
    } else {
      await api.upsertMapping(assetName, profileId);
    }
    onChanged();
  }

  async function setAccountProfile(accountId: number, profileName: string | null) {
    // Use "" to clear the return_profile (see NULLIF in backend).
    await api.updateAccount(accountId, {
      return_profile: profileName ?? "",
    });
    onChanged();
  }

  async function applyAllSuggestions() {
    const pending: Array<Promise<unknown>> = [];
    for (const row of rows) {
      if (row.mapping || row.suggestedProfileId === undefined) continue;
      pending.push(api.upsertMapping(row.name, row.suggestedProfileId));
    }
    if (pending.length === 0) return;
    await Promise.all(pending);
    onChanged();
  }

  const hasSuggestions = rows.some(
    (r) => !r.mapping && r.suggestedProfileId !== undefined
  );

  if (profiles.length === 0) {
    return (
      <div className="p-4 bg-yellow-50 border border-yellow-200 rounded-lg text-sm text-yellow-800">
        Create at least one return profile to start mapping assets.
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Asset mappings */}
      <section>
        <div className="flex items-center justify-between mb-3">
          <h3 className="text-lg font-semibold text-gray-700">Asset Mappings</h3>
          {hasSuggestions && (
            <button
              onClick={applyAllSuggestions}
              className="px-3 py-1.5 text-sm border border-blue-300 text-blue-700 rounded-md hover:bg-blue-50 transition-colors"
            >
              Apply All Suggestions
            </button>
          )}
        </div>
        {rows.length === 0 ? (
          <p className="text-gray-500 text-sm">
            Add holdings to investment accounts to see asset mappings.
          </p>
        ) : (
          <div className="space-y-2">
            {rows.map((row) => (
              <AssetMappingRow
                key={row.name}
                row={row}
                profiles={profiles}
                onSet={(profileId) => setAssetMapping(row.name, profileId)}
              />
            ))}
          </div>
        )}
      </section>

      {/* Account-level profile mappings */}
      {mappableAccounts.length > 0 && (
        <section>
          <h3 className="text-lg font-semibold text-gray-700 mb-3">
            Account Profiles
          </h3>
          <p className="text-xs text-gray-500 mb-2">
            Retirement accounts and other containers use a single return profile
            (not per-holding).
          </p>
          <div className="space-y-2">
            {mappableAccounts.map((acct) => (
              <AccountProfileRow
                key={acct.id}
                account={acct}
                profiles={profiles}
                onSet={(profileName) => setAccountProfile(acct.id, profileName)}
              />
            ))}
          </div>
        </section>
      )}
    </div>
  );
}

function AssetMappingRow({
  row,
  profiles,
  onSet,
}: {
  row: AssetRow;
  profiles: ReturnProfile[];
  onSet: (profileId: number | null) => Promise<void>;
}) {
  const unmapped = !row.mapping;
  const hasSuggestion = unmapped && !!row.suggestedProfileName;

  return (
    <div
      className={`flex items-center justify-between p-3 bg-white border rounded-lg ${
        unmapped
          ? hasSuggestion
            ? "border-blue-200"
            : "border-red-200"
          : "border-gray-200"
      }`}
    >
      <div className="min-w-0 flex-1">
        <span className="font-medium text-gray-900">{row.name}</span>
        {hasSuggestion && (
          <span className="ml-2 text-xs text-blue-600">
            suggested: {row.suggestedProfileName}
          </span>
        )}
      </div>
      <div className="flex items-center gap-2">
        <select
          value={row.mapping?.profile_id ?? ""}
          onChange={(e) => {
            const v = e.target.value;
            onSet(v === "" ? null : Number(v));
          }}
          className="px-2 py-1 text-sm border border-gray-300 rounded-md bg-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        >
          <option value="">— Unmapped —</option>
          {profiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}

function AccountProfileRow({
  account,
  profiles,
  onSet,
}: {
  account: Account;
  profiles: ReturnProfile[];
  onSet: (profileName: string | null) => Promise<void>;
}) {
  const unmapped = !account.return_profile;
  return (
    <div
      className={`flex items-center justify-between p-3 bg-white border rounded-lg ${
        unmapped ? "border-red-200" : "border-gray-200"
      }`}
    >
      <div className="min-w-0 flex-1">
        <span className="font-medium text-gray-900">{account.name}</span>
      </div>
      <select
        value={account.return_profile ?? ""}
        onChange={(e) => {
          const v = e.target.value;
          onSet(v === "" ? null : v);
        }}
        className="px-2 py-1 text-sm border border-gray-300 rounded-md bg-white focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
      >
        <option value="">— Unmapped —</option>
        {profiles.map((p) => (
          <option key={p.id} value={p.name}>
            {p.name}
          </option>
        ))}
      </select>
    </div>
  );
}
