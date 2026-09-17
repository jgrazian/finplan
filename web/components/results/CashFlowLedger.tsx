"use client";

import { useEffect, useMemo, useState } from "react";
import { Blueprint, Table, Td, Th } from "@/components/ui";
import { api } from "@/lib/api/client";
import { fmtCurrency } from "@/lib/format";
import type {
  LedgerCategory,
  LedgerEntry,
  LedgerFilter,
  YearlyCashFlow,
} from "@/lib/types";
import { toLedgerEntries } from "@/lib/view/ledger";
import { type ColumnOption, ColumnPicker, useStoredColumns } from "./ColumnPicker";

/** A column of the year table. `year` is fixed; everything else can be hidden. */
type ColumnKey =
  | "year"
  | "age"
  | "income"
  | "expenses"
  | "contributions"
  | "withdrawals"
  | "appreciation"
  | "taxes"
  | "netCashFlow"
  | "netWorth";

interface Column extends ColumnOption<ColumnKey> {
  align: "left" | "right";
  width?: number;
  render: (row: YearlyCashFlow) => string;
}

const COLUMNS: readonly Column[] = [
  {
    key: "year",
    label: "Year",
    note: "calendar",
    fixed: true,
    align: "left",
    width: 58,
    render: (r) => String(r.year),
  },
  { key: "age", label: "Age", align: "right", width: 48, render: (r) => r.age == null ? "—" : String(r.age) },
  {
    key: "income",
    label: "Income",
    note: "earned",
    align: "right",
    render: (r) => fmtCurrency(r.income),
  },
  {
    key: "expenses",
    label: "Spending",
    note: "paid out",
    align: "right",
    render: (r) => fmtCurrency(r.expenses),
  },
  {
    key: "contributions",
    label: "Contrib",
    note: "into investments",
    align: "right",
    render: (r) => fmtCurrency(r.contributions),
  },
  {
    key: "withdrawals",
    label: "Withdraw",
    note: "out of investments",
    align: "right",
    render: (r) => fmtCurrency(r.withdrawals),
  },
  {
    key: "appreciation",
    label: "Growth",
    note: "interest on cash",
    align: "right",
    render: (r) => fmtCurrency(r.appreciation),
  },
  {
    key: "taxes",
    label: "Taxes",
    note: "plus penalties",
    align: "right",
    render: (r) => fmtCurrency(r.taxes),
  },
  {
    key: "netCashFlow",
    label: "Net",
    note: "in less out",
    align: "right",
    render: (r) => fmtCurrency(r.netCashFlow),
  },
  {
    key: "netWorth",
    label: "Net worth",
    note: "at year end",
    align: "right",
    render: (r) => fmtCurrency(r.netWorth),
  },
];

/**
 * What the table opens showing, until someone picks otherwise: the shape of a
 * year, without the detail.
 */
const DEFAULT_COLUMNS: ColumnKey[] = [
  "year",
  "age",
  "income",
  "expenses",
  "withdrawals",
  "taxes",
  "netWorth",
];

/** Where the picked columns are remembered, per browser. */
const COLUMN_STORE = "finplan.results.cashflow.columns";

const FILTERS: ReadonlyArray<{ value: LedgerFilter; label: string }> = [
  { value: "all", label: "All" },
  { value: "cash", label: "Cash" },
  { value: "asset", label: "Assets" },
  { value: "tax", label: "Taxes" },
  { value: "event", label: "Events" },
];

/** Tone per bucket. The palette is monochrome, so these are values, not hues. */
const CATEGORY_COLOR: Record<LedgerCategory, string> = {
  cash: "var(--color-accent-700)",
  asset: "var(--color-accent-500)",
  tax: "var(--color-neutral-800)",
  event: "var(--color-neutral-600)",
};

const MUTED = "color-mix(in srgb, var(--color-text) 52%, transparent)";
const FAINT = "color-mix(in srgb, var(--color-text) 40%, transparent)";

function countFor(row: YearlyCashFlow, filter: LedgerFilter): number {
  return filter === "all" ? row.ledger.total : row.ledger[filter];
}

/**
 * The Results screen's lower pane (canvas 9b).
 *
 * One table rather than two. The year table holds the totals; the ledger that
 * used to sit beside it now nests under each expanded year, which is the only
 * arrangement where a figure and the effects that produced it are ever on
 * screen at the same time. Any number of years can be open at once, so two
 * years can be read against each other without closing either.
 *
 * Every figure is real — the plan's first-year dollars — because the mapper
 * upstream deflated them, entries included, at the year each was recorded.
 */
export function CashFlowLedger({
  rows,
  series,
  pathLabel,
  runId,
  dollarLabel,
}: {
  rows: YearlyCashFlow[];
  series: string;
  pathLabel: string;
  /** The run whose ledger an expanded year reads from. */
  runId: number | undefined;
  /** Explicit real base date, or nominal units for historical data. */
  dollarLabel: string;
}) {
  const [visible, setVisible] = useStoredColumns(COLUMN_STORE, COLUMNS, DEFAULT_COLUMNS);
  const [filter, setFilter] = useState<LedgerFilter>("all");
  const [openYears, setOpenYears] = useState<ReadonlySet<number>>(() => new Set());

  const columns = useMemo(() => COLUMNS.filter((c) => visible.has(c.key)), [visible]);

  const hasLedger = rows.some((r) => r.ledger.total > 0);

  const toggleYear = (year: number) =>
    setOpenYears((prev) => {
      const next = new Set(prev);
      if (!next.delete(year)) next.add(year);
      return next;
    });

  return (
    <Blueprint style={{ padding: "18px 22px 20px" }}>
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 16,
          marginBottom: 10,
          flexWrap: "wrap",
        }}
      >
        <div
          style={{
            display: "flex",
            alignItems: "baseline",
            gap: 10,
            flexWrap: "wrap",
            minWidth: 0,
            flex: "1 1 340px",
          }}
        >
          <h6 style={{ margin: 0 }}>Cash flow — {pathLabel}</h6>
          <span style={{ fontSize: 11, color: MUTED }}>
            {rows.length} years · {dollarLabel} ·{" "}
            {hasLedger
              ? "expand a year to read the effects that produced its numbers"
              : "this scenario is not collecting a ledger, so there is nothing to expand"}
          </span>
        </div>

        {/* The two controls travel together, pinned to the right edge: they
            wrap onto a line of their own rather than leaving the picker
            stranded under the title. */}
        <div
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "flex-end",
            gap: 8,
            marginLeft: "auto",
            flexWrap: "wrap",
          }}
        >
          {hasLedger && (
            <div style={{ display: "flex", gap: 4 }}>
              {FILTERS.map((f) => (
                <button
                  key={f.value}
                  type="button"
                  className="sbtn"
                  aria-pressed={filter === f.value}
                  onClick={() => setFilter(f.value)}
                  style={
                    filter === f.value
                      ? { background: "var(--color-accent)", color: "var(--color-bg)" }
                      : { color: MUTED }
                  }
                >
                  {f.label}
                </button>
              ))}
            </div>
          )}

          <ColumnPicker options={COLUMNS} visible={visible} onChange={setVisible} />
        </div>
      </div>

      <Blueprint style={{ padding: "0 2px 2px" }}>
        <div style={{ maxHeight: 560, overflow: "auto" }}>
          <Table compact>
            <thead>
              <tr>
                {hasLedger && <Th style={{ ...STICKY, width: 22 }} />}
                {columns.map((c) => (
                  <Th
                    key={c.key}
                    align={c.align}
                    style={{ ...STICKY, width: c.width, whiteSpace: "nowrap" }}
                  >
                    {c.label}
                  </Th>
                ))}
                {hasLedger && (
                  <Th align="right" style={{ ...STICKY, whiteSpace: "nowrap" }}>
                    Ledger
                  </Th>
                )}
              </tr>
            </thead>
            <tbody>
              {rows.map((row) => {
                const count = countFor(row, filter);
                // A year the filter has emptied reads as closed rather than as
                // an empty drawer — and reopens on its own once the filter
                // that emptied it is lifted.
                const open = openYears.has(row.year) && count > 0;
                return (
                  <YearRow
                    key={row.year}
                    row={row}
                    columns={columns}
                    hasLedger={hasLedger}
                    count={count}
                    open={open}
                    span={columns.length + (hasLedger ? 2 : 0)}
                    runId={runId}
                    series={series}
                    filter={filter}
                    onToggle={() => toggleYear(row.year)}
                  />
                );
              })}
            </tbody>
          </Table>
        </div>
      </Blueprint>

      <div style={{ fontSize: 11, marginTop: 6, color: MUTED }}>
        Withdraw, contrib and spending are the sums of every effect that fired that
        year — expand a row to itemise them. The filter chips scope the nested ledger.
      </div>
    </Blueprint>
  );
}

const STICKY = {
  position: "sticky" as const,
  top: 0,
  zIndex: 1,
  background: "var(--color-bg)",
};

/** The rule marking a year an event fired in, so it is findable when closed. */
const MILESTONE = {
  boxShadow: "inset 2px 0 0 color-mix(in srgb, var(--color-accent) 45%, transparent)",
};

const EXPANDED = {
  background: "color-mix(in srgb, var(--color-accent) 14%, transparent)",
  boxShadow: "inset 2px 0 0 var(--color-accent)",
};

function YearRow({
  row,
  columns,
  hasLedger,
  count,
  open,
  span,
  runId,
  series,
  filter,
  onToggle,
}: {
  row: YearlyCashFlow;
  columns: readonly Column[];
  hasLedger: boolean;
  count: number;
  open: boolean;
  span: number;
  runId: number | undefined;
  /** The path the row's figures came from, so its drawer reads the same one. */
  series: string;
  filter: LedgerFilter;
  onToggle: () => void;
}) {
  return (
    <>
      <tr
        className={hasLedger && count > 0 ? "rowsel" : undefined}
        onClick={hasLedger && count > 0 ? onToggle : undefined}
        aria-expanded={hasLedger && count > 0 ? open : undefined}
        style={open ? EXPANDED : row.ledger.tag ? MILESTONE : undefined}
      >
        {hasLedger && (
          <Td
            style={{
              fontFamily: "ui-monospace, Menlo, monospace",
              fontSize: 11,
              color: FAINT,
            }}
          >
            {count > 0 ? (open ? "▾" : "▸") : ""}
          </Td>
        )}
        {columns.map((c) => (
          <Td key={c.key} align={c.align} style={{ whiteSpace: "nowrap" }}>
            {c.render(row)}
          </Td>
        ))}
        {hasLedger && (
          <Td align="right" style={{ whiteSpace: "nowrap" }}>
            {row.ledger.tag && (
              // The event's own name, as the user wrote it — so no case
              // transform — clipped rather than allowed to widen the column.
              <span
                className="tag tag-outline"
                title={row.ledger.tag}
                style={{
                  // `.tag` is inline-flex, where an ellipsis has nothing to
                  // apply to; inline-block gives the text a block box to be
                  // clipped in.
                  display: "inline-block",
                  marginRight: 8,
                  maxWidth: 160,
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  whiteSpace: "nowrap",
                  verticalAlign: "middle",
                }}
              >
                {row.ledger.tag}
              </span>
            )}
            <span
              style={{
                fontFamily: "ui-monospace, Menlo, monospace",
                fontSize: 11,
                color: MUTED,
              }}
            >
              {count}
            </span>
          </Td>
        )}
      </tr>

      {open && (
        <tr>
          <td
            colSpan={span}
            style={{
              padding: "0 0 0 22px",
              background: "color-mix(in srgb, var(--color-accent) 6%, transparent)",
            }}
          >
            <div
              style={{
                borderLeft: "2px solid var(--color-accent)",
                padding: "6px 10px 10px 12px",
              }}
            >
              <LedgerDrawer
                runId={runId}
                series={series}
                year={row.year}
                factor={row.inflationFactor}
                filter={filter}
              />
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

/**
 * One expanded year's entries.
 *
 * The fetch lives here rather than in the table so that any number of years
 * can be open at once — each drawer asks for its own year when it mounts, and
 * a year that is closed again stops being asked for at all.
 */
function LedgerDrawer({
  runId,
  series,
  year,
  factor,
  filter,
}: {
  runId: number | undefined;
  series: string;
  year: number;
  /** The year's cumulative inflation, so items and totals share dollars. */
  factor: number;
  filter: LedgerFilter;
}) {
  const state = useLedgerYear(runId, series, year, factor, filter);
  if (state.error) {
    return (
      <p style={{ margin: 0, fontSize: 12, color: MUTED }}>
        The ledger for {year} could not be read — {state.error}
      </p>
    );
  }
  if (state.loading) {
    return (
      <p style={{ margin: 0, fontSize: 12, color: MUTED }}>Reading {year}…</p>
    );
  }
  if (state.entries.length === 0) {
    return (
      <p style={{ margin: 0, fontSize: 12, color: MUTED }}>
        Nothing in {year} under this filter.
      </p>
    );
  }

  const shown = state.entries.length;
  const hidden = state.total - shown;

  return (
    <>
      <div style={{ display: "flex", flexDirection: "column" }}>
        {state.entries.map((entry) => (
          <EntryRow key={entry.id} entry={entry} />
        ))}
      </div>
      <div
        style={{
          display: "flex",
          gap: 10,
          alignItems: "center",
          fontSize: 11,
          marginTop: 8,
          color: MUTED,
        }}
      >
        <span>
          {shown} {filter === "all" ? "entries" : `${filter} entries`}
          {hidden > 0 ? ` · ${hidden} more not shown` : ""}
        </span>
        <button
          type="button"
          className="sbtn"
          style={{ marginLeft: "auto" }}
          onClick={() => void copyEntries(state.entries)}
        >
          Copy
        </button>
        <button
          type="button"
          className="sbtn"
          onClick={() => downloadCsv(state.entries, year)}
        >
          Export CSV
        </button>
      </div>
    </>
  );
}

function EntryRow({ entry }: { entry: LedgerEntry }) {
  return (
    <div
      style={{
        display: "grid",
        gridTemplateColumns: "84px 132px 1fr auto",
        gap: 8,
        alignItems: "baseline",
        // A printout, not prose: the whole line is monospace so dates, figures
        // and the columns between them stay in register down the drawer.
        fontFamily: "ui-monospace, Menlo, monospace",
        fontSize: 11,
        lineHeight: 1.45,
        padding: "3px 5px",
        // Ruled above rather than below, so the last entry does not carry a
        // line into the footer beneath it.
        borderTop: "1px solid color-mix(in srgb, var(--color-text) 7%, transparent)",
      }}
    >
      <span style={{ color: FAINT }}>{entry.date}</span>
      <span
        style={{
          fontFamily: "var(--font-heading)",
          fontWeight: 600,
          fontSize: 11,
          letterSpacing: "0.05em",
          textTransform: "uppercase",
          color: CATEGORY_COLOR[entry.category],
        }}
      >
        {entry.kind}
      </span>
      <span>
        {entry.detail}
        {entry.basis != null && (
          <span style={{ color: MUTED }}>
            {" "}
            · {entry.basisLabel ?? "on"} {fmtCurrency(entry.basis)}
          </span>
        )}
      </span>
      <span style={{ textAlign: "right", whiteSpace: "nowrap" }}>
        {entry.amount == null ? "" : fmtCurrency(entry.amount)}
      </span>
    </div>
  );
}

// ── the expanded year's entries ─────────────────────────────────────────────

interface LedgerState {
  entries: LedgerEntry[];
  total: number;
  loading: boolean;
  error?: string;
}

const IDLE: LedgerState = { entries: [], total: 0, loading: false };

/** What one completed fetch produced, tagged with the request that asked for it. */
interface Loaded {
  runId: number;
  series: string;
  year: number;
  category?: LedgerCategory;
  entries: LedgerEntry[];
  total: number;
  error?: string;
}

/**
 * The entries behind one expanded year, fetched when its drawer mounts.
 *
 * Deflated by that year's own factor — the same one the row above was deflated
 * by — so the items and the total they sum to are quoted in the same dollars.
 *
 * Loading is derived from whether what has arrived answers what is currently
 * being asked, rather than flagged on the way out: that way a year opened and
 * closed while its fetch was in flight cannot leave the next one showing the
 * wrong entries.
 */
function useLedgerYear(
  runId: number | undefined,
  series: string,
  year: number,
  factor: number,
  filter: LedgerFilter,
): LedgerState {
  const [loaded, setLoaded] = useState<Loaded>();
  const category = filter === "all" ? undefined : filter;

  useEffect(() => {
    if (runId == null) return;
    let live = true;

    api.runs
      .ledger(runId, { series, year, category })
      .then((page) => {
        if (!live) return;
        if (page.run_id !== runId || page.series_id !== series) {
          throw new Error("Ledger response does not match the selected path");
        }
        setLoaded({
          runId,
          series,
          year,
          category,
          entries: toLedgerEntries(page, factor),
          total: page.total,
        });
      })
      .catch((err: Error) => {
        if (live) {
          setLoaded({
            runId,
            series,
            year,
            category,
            entries: [],
            total: 0,
            error: err.message,
          });
        }
      });

    return () => {
      live = false;
    };
  }, [category, factor, runId, series, year]);

  if (runId == null) return IDLE;
  if (
    loaded?.runId !== runId ||
    loaded.series !== series ||
    loaded.year !== year ||
    loaded.category !== category
  ) {
    return { entries: [], total: 0, loading: true };
  }
  return {
    entries: loaded.entries,
    total: loaded.total,
    loading: false,
    error: loaded.error,
  };
}

// ── export ──────────────────────────────────────────────────────────────────

const HEADERS = ["date", "category", "kind", "detail", "amount", "basis_label", "basis"];

function toRows(entries: LedgerEntry[]): string[][] {
  return entries.map((e) => [
    e.date,
    e.category,
    e.kind,
    e.detail,
    e.amount == null ? "" : e.amount.toFixed(2),
    e.basisLabel ?? "",
    e.basis == null ? "" : e.basis.toFixed(2),
  ]);
}

/** Tab-separated, so it lands in a spreadsheet as columns rather than one cell. */
async function copyEntries(entries: LedgerEntry[]) {
  const text = [HEADERS, ...toRows(entries)].map((r) => r.join("\t")).join("\n");
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    // A denied clipboard permission is the user's answer, not an error worth
    // interrupting them over.
  }
}

function csvCell(value: string): string {
  return /[",\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

function downloadCsv(entries: LedgerEntry[], year: number) {
  const csv = [HEADERS, ...toRows(entries)]
    .map((row) => row.map(csvCell).join(","))
    .join("\n");
  const url = URL.createObjectURL(new Blob([csv], { type: "text/csv;charset=utf-8" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = `ledger-${year}.csv`;
  link.click();
  URL.revokeObjectURL(url);
}
