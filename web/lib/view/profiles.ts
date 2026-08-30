/**
 * `api::profiles::Profile` → the Profiles screen's row types.
 *
 * A distribution is a tagged tree on the wire; the tables want a mean and a
 * spread. Two shapes have neither in closed form — a bootstrap resamples a real
 * history, and a regime model blends two distributions — so those report null
 * and the UI shows a dash instead of a fabricated figure.
 */
import type { DistributionSpec, Profile } from "@/lib/api/types";
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
    const { mean, sd, source } = summarize(profile.distribution);
    return {
      id: profile.name,
      serverId: profile.id,
      kind: profile.distribution.kind,
      source: profile.description ?? source,
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
      mean,
      sd,
      note: profile.description ?? source,
    };
  });
}

/**
 * A resampled history has no parameters to edit — its shape comes from the
 * data. Duplicating one yields an editable copy.
 */
export function isReadOnlyPreset(profile: ReturnProfile): boolean {
  return profile.kind === "Bootstrap";
}
