/**
 * The plan-scoped groups of `PlanApi` (assets, accounts, positions, events,
 * parameters, expressions, history presets), each an `EditOp` through
 * `Core.editPlan` followed by the read the route's response body is.
 */
import type {
  AccountsApi,
  AssetsApi,
  EventsApi,
  ExpressionsApi,
  ParametersApi,
} from "../api/plan.ts";
import type {
  Account,
  Asset,
  Event,
  ExpressionValidation,
  HistoryPreset,
  NamedParameter,
  Position,
} from "../api/types.ts";
import type { Core } from "./core.ts";
import { guard, notFound } from "./errors.ts";

export function assetsGroup(core: Core): AssetsApi {
  const edit = <T>(id: number, op: object, then?: (created: number | null) => object | undefined) =>
    core.editPlan<T>(id, op, then);
  return {
    list: (scenarioId) => core.read<Asset[]>(scenarioId, { query: "assets" }),
    create: async (scenarioId, body) =>
      (await edit<Asset>(scenarioId, { op: "create_asset", body }, (id) => ({ query: "asset", id }))).read,
    update: async (scenarioId, id, body) =>
      (await edit<Asset>(scenarioId, { op: "update_asset", id, body }, () => ({ query: "asset", id }))).read,
    remove: async (scenarioId, id) => {
      await edit(scenarioId, { op: "delete_asset", id });
    },
    reorder: async (scenarioId, ids) => {
      await edit(scenarioId, { op: "reorder_assets", ids });
    },
  };
}

export function accountsGroup(core: Core): AccountsApi {
  const position = async (scenarioId: number, accountId: number, id: number | null): Promise<Position> => {
    const positions = await core.read<Position[]>(scenarioId, { query: "positions", account_id: accountId });
    const found = positions.find((p) => p.id === id);
    if (!found) throw notFound("position");
    return found;
  };
  return {
    list: (scenarioId) => core.read<Account[]>(scenarioId, { query: "accounts" }),
    create: async (scenarioId, body) =>
      (await core.editPlan<Account>(scenarioId, { op: "create_account", body }, (id) => ({ query: "account", id })))
        .read,
    update: async (scenarioId, id, body) =>
      (await core.editPlan<Account>(scenarioId, { op: "update_account", id, body }, () => ({ query: "account", id })))
        .read,
    remove: async (scenarioId, id) => {
      await core.editPlan(scenarioId, { op: "delete_account", id });
    },
    reorder: async (scenarioId, ids) => {
      await core.editPlan(scenarioId, { op: "reorder_accounts", ids });
    },
    positions: (scenarioId, id) => core.read<Position[]>(scenarioId, { query: "positions", account_id: id }),
    addPosition: async (scenarioId, id, body) => {
      const { id: created } = await core.editPlan(scenarioId, { op: "create_position", account_id: id, body });
      return position(scenarioId, id, created);
    },
    updatePosition: async (scenarioId, id, positionId, body) => {
      await core.editPlan(scenarioId, { op: "update_position", account_id: id, id: positionId, body });
      return position(scenarioId, id, positionId);
    },
    removePosition: async (scenarioId, id, positionId) => {
      await core.editPlan(scenarioId, { op: "delete_position", account_id: id, id: positionId });
    },
    reorderPositions: async (scenarioId, id, ids) => {
      await core.editPlan(scenarioId, { op: "reorder_positions", account_id: id, ids });
    },
  };
}

export function eventsGroup(core: Core): EventsApi {
  return {
    list: (scenarioId) => core.read<Event[]>(scenarioId, { query: "events" }),
    create: async (scenarioId, body) =>
      (await core.editPlan<Event>(scenarioId, { op: "create_event", body }, (id) => ({ query: "event", id }))).read,
    replace: async (scenarioId, id, body) =>
      (await core.editPlan<Event>(scenarioId, { op: "replace_event", id, body }, () => ({ query: "event", id })))
        .read,
    remove: async (scenarioId, id) => {
      await core.editPlan(scenarioId, { op: "delete_event", id });
    },
    reorder: async (scenarioId, ids) => {
      await core.editPlan(scenarioId, { op: "reorder_events", ids });
    },
  };
}

export function parametersGroup(core: Core): ParametersApi {
  const parameter = async (scenarioId: number, id: number | null): Promise<NamedParameter> => {
    const all = await core.read<NamedParameter[]>(scenarioId, { query: "parameters" });
    const found = all.find((p) => p.id === id);
    if (!found) throw notFound("parameter");
    return found;
  };
  return {
    list: (scenarioId) => core.read<NamedParameter[]>(scenarioId, { query: "parameters" }),
    create: async (scenarioId, body) => {
      const { id } = await core.editPlan(scenarioId, { op: "create_parameter", body });
      return parameter(scenarioId, id);
    },
    update: async (scenarioId, id, body) => {
      await core.editPlan(scenarioId, { op: "update_parameter", id, body });
      return parameter(scenarioId, id);
    },
    remove: async (scenarioId, id) => {
      await core.editPlan(scenarioId, { op: "delete_parameter", id });
    },
  };
}

export function expressionsGroup(core: Core): ExpressionsApi {
  return {
    validate: (scenarioId, body) =>
      core.read<ExpressionValidation>(scenarioId, { query: "validate_expression", request: body }),
  };
}

/** `GET /history-presets`: the bootstrap histories the engine ships with. Plan-independent. */
export async function historyPresets(core: Core): Promise<HistoryPreset[]> {
  return core.store.transact("r", async (tx) => {
    // The engine's read takes a plan, though this one does not look at it: use
    // the first, or a throwaway when the device has none.
    const plans = await tx.listPlans();
    const library = await core.library(tx);
    const graph =
      plans[0]?.graph ??
      guard(() =>
        core.engine.new_plan(
          JSON.stringify({ name: "presets", start_date: "2026-01-01", duration_years: 1 }),
          library,
          0,
          "2026-01-01 00:00:00",
        ),
      );
    return core.call<HistoryPreset[]>((engine) =>
      engine.read(graph, library, JSON.stringify({ query: "history_presets" })),
    );
  });
}
