"use client";

import { useState } from "react";
import type { ReturnProfile, ReturnProfileType } from "@/lib/types";
import { RETURN_PROFILE_TYPE_LABELS } from "@/lib/types";
import type { ProfilePayload } from "@/lib/api";

interface ProfileFormProps {
  initial?: ReturnProfile;
  onSubmit: (data: ProfilePayload) => Promise<void>;
  onCancel: () => void;
}

type DfPreset = "5" | "3" | "2";

const DF_LABELS: Record<DfPreset, string> = {
  "5": "Moderate tails (df=5)",
  "3": "Fat tails (df=3)",
  "2": "Very fat tails (df=2)",
};

// Defaults match crates/finplan/src/actions/profile.rs
const DEFAULT_PARAMS: Record<
  ReturnProfileType,
  { rate?: number; mean?: number; std_dev?: number; scale?: number; df?: number }
> = {
  None: {},
  Fixed: { rate: 0.07 },
  Normal: { mean: 0.07, std_dev: 0.15 },
  LogNormal: { mean: 0.07, std_dev: 0.15 },
  StudentT: { mean: 0.0957, scale: 0.1652, df: 5 },
};

function toPct(n: number | undefined): string {
  if (n === undefined || n === null) return "";
  return (n * 100).toString();
}

function parsePct(s: string): number | undefined {
  if (!s.trim()) return undefined;
  const n = parseFloat(s);
  return Number.isFinite(n) ? n / 100 : undefined;
}

export function ProfileForm({ initial, onSubmit, onCancel }: ProfileFormProps) {
  const isEditing = !!initial;
  const [profileType, setProfileType] = useState<ReturnProfileType | null>(
    initial?.profile_type ?? null
  );
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");

  const defaults = profileType ? DEFAULT_PARAMS[profileType] : {};
  const [rate, setRate] = useState(
    toPct(initial?.rate ?? defaults.rate)
  );
  const [mean, setMean] = useState(
    toPct(initial?.mean ?? defaults.mean)
  );
  const [stdDev, setStdDev] = useState(
    toPct(initial?.std_dev ?? defaults.std_dev)
  );
  const [scale, setScale] = useState(
    toPct(initial?.scale ?? defaults.scale)
  );
  const [df, setDf] = useState<DfPreset>(
    initial?.df === 2 ? "2" : initial?.df === 3 ? "3" : "5"
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState("");

  function onPickType(t: ReturnProfileType) {
    const d = DEFAULT_PARAMS[t];
    setProfileType(t);
    setRate(toPct(d.rate));
    setMean(toPct(d.mean));
    setStdDev(toPct(d.std_dev));
    setScale(toPct(d.scale));
    if (t === "StudentT") setDf("5");
  }

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!profileType || !name.trim()) return;

    const payload: ProfilePayload = {
      name: name.trim(),
      description: description.trim() || undefined,
      profile_type: profileType,
    };

    if (profileType === "Fixed") {
      payload.rate = parsePct(rate);
    } else if (profileType === "Normal" || profileType === "LogNormal") {
      payload.mean = parsePct(mean);
      payload.std_dev = parsePct(stdDev);
    } else if (profileType === "StudentT") {
      payload.mean = parsePct(mean);
      payload.scale = parsePct(scale);
      payload.df = parseFloat(df);
    }

    setSubmitting(true);
    setError("");
    try {
      await onSubmit(payload);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Something went wrong");
    } finally {
      setSubmitting(false);
    }
  }

  // Step 1: type picker when creating
  if (!isEditing && !profileType) {
    return (
      <div className="space-y-4">
        <h3 className="text-lg font-semibold">Select Profile Type</h3>
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {(Object.keys(RETURN_PROFILE_TYPE_LABELS) as ReturnProfileType[]).map(
            (t) => (
              <button
                key={t}
                type="button"
                onClick={() => onPickType(t)}
                className="p-3 border border-gray-200 rounded-lg hover:border-blue-400 hover:bg-blue-50 transition-colors text-left"
              >
                {RETURN_PROFILE_TYPE_LABELS[t]}
              </button>
            )
          )}
        </div>
        <button
          type="button"
          onClick={onCancel}
          className="text-sm text-gray-500 hover:text-gray-700"
        >
          Cancel
        </button>
      </div>
    );
  }

  return (
    <form onSubmit={handleSubmit} className="space-y-4">
      <h3 className="text-lg font-semibold">
        {isEditing ? "Edit" : "New"}{" "}
        {profileType ? RETURN_PROFILE_TYPE_LABELS[profileType] : "Profile"}
      </h3>

      {error && (
        <div className="p-3 bg-red-50 border border-red-200 rounded text-red-700 text-sm">
          {error}
        </div>
      )}

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Name
        </label>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          required
        />
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-700 mb-1">
          Description
        </label>
        <input
          type="text"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>

      {profileType === "Fixed" && (
        <div>
          <label className="block text-sm font-medium text-gray-700 mb-1">
            Rate (%)
          </label>
          <input
            type="number"
            step="0.01"
            value={rate}
            onChange={(e) => setRate(e.target.value)}
            className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            required
          />
        </div>
      )}

      {(profileType === "Normal" || profileType === "LogNormal") && (
        <div className="grid grid-cols-2 gap-3">
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Mean (%)
            </label>
            <input
              type="number"
              step="0.01"
              value={mean}
              onChange={(e) => setMean(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
              required
            />
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Std Dev (%)
            </label>
            <input
              type="number"
              step="0.01"
              value={stdDev}
              onChange={(e) => setStdDev(e.target.value)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
              required
            />
          </div>
        </div>
      )}

      {profileType === "StudentT" && (
        <>
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Mean (%)
              </label>
              <input
                type="number"
                step="0.01"
                value={mean}
                onChange={(e) => setMean(e.target.value)}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                required
              />
            </div>
            <div>
              <label className="block text-sm font-medium text-gray-700 mb-1">
                Scale (%)
              </label>
              <input
                type="number"
                step="0.01"
                value={scale}
                onChange={(e) => setScale(e.target.value)}
                className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
                required
              />
            </div>
          </div>
          <div>
            <label className="block text-sm font-medium text-gray-700 mb-1">
              Tail Behavior
            </label>
            <select
              value={df}
              onChange={(e) => setDf(e.target.value as DfPreset)}
              className="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent bg-white"
            >
              {(Object.keys(DF_LABELS) as DfPreset[]).map((opt) => (
                <option key={opt} value={opt}>
                  {DF_LABELS[opt]}
                </option>
              ))}
            </select>
          </div>
        </>
      )}

      <div className="flex gap-3 pt-2">
        <button
          type="submit"
          disabled={submitting}
          className="px-4 py-2 bg-blue-600 text-white rounded-md hover:bg-blue-700 disabled:opacity-50 transition-colors"
        >
          {submitting ? "Saving..." : isEditing ? "Save" : "Create"}
        </button>
        <button
          type="button"
          onClick={isEditing ? onCancel : () => setProfileType(null)}
          className="px-4 py-2 border border-gray-300 rounded-md hover:bg-gray-50 transition-colors"
        >
          {isEditing ? "Cancel" : "Back"}
        </button>
      </div>
    </form>
  );
}
