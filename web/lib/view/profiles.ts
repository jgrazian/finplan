/**
 * `api::profiles::Profile` → the Profiles screen's row types.
 *
 * A distribution is a tagged tree on the wire; the tables want a mean and a
 * spread. Two shapes have neither in closed form — a bootstrap resamples a real
 * history, and a regime model blends two distributions — so those report null
 * and the UI shows a dash instead of a fabricated figure.
 */
import type { DistributionSpec, HistoryPreset, Profile } from "@/lib/api/types";
import { historyStats } from "@/lib/history";
import type { InflationProfile, ReturnProfile } from "@/lib/types";

interface Summary {
  mean: number | null;
  sd: number | null;
  source: string;
}

function summarize(distribution: DistributionSpec): Summary {
  switch (distribution.kind) {
    case "None":
      return { mean: 0, sd: 0, source: "no growth" };
    case "Fixed":
      return { mean: distribution.rate * 100, sd: 0, source: "constant" };
    case "Normal":
      return {
        mean: distribution.mean * 100,
        sd: distribution.std_dev * 100,
        source: "normal draws",
      };
    case "LogNormal":
      return {
        mean: distribution.mean * 100,
        sd: distribution.std_dev * 100,
        source: "log-normal draws",
      };
    case "StudentT":
      // `scale` is not a standard deviation, but for df > 2 it is the closest
      // honest analogue to report next to the mean.
      return {
        mean: distribution.mean * 100,
        sd: distribution.scale * 100,
        source: `Student-t · ${distribution.df} df`,
      };
    case "RegimeSwitching":
      return { mean: null, sd: null, source: "bull/bear regime switching" };
    case "Bootstrap":
      return {
        mean: null,
        sd: null,
        source: `resampled ${distribution.preset}${
          distribution.block_size ? ` · ${distribution.block_size}y blocks` : ""
        }`,
      };
  }
}

export function toViewReturnProfiles(profiles: Profile[]): ReturnProfile[] {
  return profiles.map((profile) => {
    const { mean, sd } = summarize(profile.distribution);
    return {
      id: profile.name,
      serverId: profile.id,
      kind: profile.distribution.kind,
      description: profile.description ?? "",
      assetClass: profile.asset_class,
      distribution: profile.distribution,
      mean,
      sd,
      usedBy: profile.used_by,
    };
  });
}

export function toViewInflationProfiles(profiles: Profile[]): InflationProfile[] {
  return profiles.map((profile) => {
    const { mean, sd, source } = summarize(profile.distribution);
    return {
      id: profile.name,
      serverId: profile.id,
      kind: profile.distribution.kind,
      distribution: profile.distribution,
      mean,
      sd,
      note: profile.description ?? source,
    };
  });
}

/**
 * Attaches the years behind each `Bootstrap` profile, and the figures they
 * imply.
 *
 * `summarize` reports null for a resampled history because the wire format on
 * its own carries only a preset name. Once the preset table has loaded the
 * client holds the observations, and a mean and a spread it measured are worth
 * more than a dash — they are facts about the history, not a fitted curve. The
 * profile still draws from the years, which is why they are carried too.
 */
export function withHistories(
  profiles: ReturnProfile[],
  presets: HistoryPreset[],
): ReturnProfile[] {
  if (presets.length === 0) return profiles;
  const byId = new Map(presets.map((p) => [p.id, p.returns]));
  return profiles.map((profile) => {
    if (profile.distribution.kind !== "Bootstrap") return profile;
    const history = byId.get(profile.distribution.preset);
    if (!history) return profile;
    const stats = historyStats(history);
    return stats ? { ...profile, history, mean: stats.mean, sd: stats.sd } : profile;
  });
}
