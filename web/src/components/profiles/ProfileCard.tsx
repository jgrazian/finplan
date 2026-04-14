"use client";

import type { ReturnProfile } from "@/lib/types";
import { formatPercent } from "@/lib/utils";

interface ProfileCardProps {
  profile: ReturnProfile;
  onEdit: () => void;
  onDelete: () => void;
}

function describeProfile(p: ReturnProfile): string {
  switch (p.profile_type) {
    case "None":
      return "0%";
    case "Fixed":
      return p.rate !== undefined ? `Rate: ${formatPercent(p.rate * 100)}` : "";
    case "Normal":
    case "LogNormal":
      return p.mean !== undefined && p.std_dev !== undefined
        ? `μ=${formatPercent(p.mean * 100)}, σ=${formatPercent(p.std_dev * 100)}`
        : "";
    case "StudentT":
      return p.mean !== undefined && p.scale !== undefined
        ? `μ=${formatPercent(p.mean * 100)}, scale=${formatPercent(
            p.scale * 100
          )}, df=${p.df ?? ""}`
        : "";
  }
}

function typeLabel(t: ReturnProfile["profile_type"]): string {
  switch (t) {
    case "None":
      return "None";
    case "Fixed":
      return "Fixed";
    case "Normal":
      return "Normal";
    case "LogNormal":
      return "Log-Normal";
    case "StudentT":
      return "Student's t";
  }
}

export function ProfileCard({ profile, onEdit, onDelete }: ProfileCardProps) {
  return (
    <div className="p-4 bg-white border border-gray-200 rounded-lg">
      <div className="flex items-start justify-between">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <h3 className="font-medium text-gray-900 truncate">
              {profile.name}
            </h3>
            <span className="inline-block px-2 py-0.5 text-xs rounded-full bg-gray-100 text-gray-600">
              {typeLabel(profile.profile_type)}
            </span>
          </div>
          <p className="mt-1 text-sm text-gray-600">{describeProfile(profile)}</p>
          {profile.description && (
            <p className="mt-1 text-sm text-gray-500">{profile.description}</p>
          )}
        </div>
        <div className="flex gap-2 ml-4">
          <button
            onClick={onEdit}
            className="text-sm text-blue-600 hover:text-blue-800"
          >
            Edit
          </button>
          <button
            onClick={onDelete}
            className="text-sm text-red-600 hover:text-red-800"
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
