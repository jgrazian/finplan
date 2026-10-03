import { localApi } from "../api/local";
import { remoteApi } from "../api/remote";
import { createPlanRouter } from "../api/route";

/**
 * The `PlanApi` for a plan ref (`l12`, `s8a4f21b7c903`) or a home. Not a hook,
 * for the places that have a ref in hand and no component: an event handler, a
 * loader that runs before the open plan is known, the scenario list.
 *
 * An unknown or absent ref is the cloud's — what every plan was before local
 * ones existed — so the app with local mode off never reaches `localApi`.
 */
export const planApiFor = createPlanRouter({ local: localApi, cloud: remoteApi });
