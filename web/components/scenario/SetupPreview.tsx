import { Fragment } from "react";
import { Blueprint, StatLabel, Table, Tag, Td, Th, type TagTone } from "@/components/ui";
import { money } from "@/lib/view/format";

export interface PreviewAccount {
  name: string;
  /** "Cash", "Retirement", "Investment": the muted word after the name. */
  kind: string;
  tax: { label: string; tone: TagTone };
  profile: string;
  balance: number;
}

/** A word of the sentence, a filled slot, or an expression slot. */
export type SentencePart = string | { slot: string } | { expr: string };

export interface PreviewEvent {
  name: string;
  parts: SentencePart[];
  /** The step on screen wrote it: marked with the accent rule and "just added". */
  fresh?: boolean;
}

// The net-worth bar's fills, in account order: cash, 401(k), other.
const FILLS = ["var(--color-accent-300)", "var(--color-accent-600)", "var(--color-accent)"];

const plural = (count: number, one: string) => `${count} ${one}${count === 1 ? "" : "s"}`;

/**
 * The plan so far (design 1a, right-hand side): the accounts and the event
 * sentences the answers create, drawn as the Portfolio and Plan tabs will
 * show them once the plan exists.
 */
export function SetupPreview({
  accounts,
  events,
  pending,
  assumptions,
  blank,
}: {
  accounts: PreviewAccount[];
  events: PreviewEvent[];
  /** What later steps will add, in words. */
  pending: string[];
  assumptions?: ReadonlyArray<{ label: string; value: string }>;
  /** A blank scenario: details only, nothing else is created. */
  blank?: boolean;
}) {
  const wealth = accounts.reduce((sum, account) => sum + account.balance, 0);
  const funded = accounts.filter((account) => account.balance > 0);
  return (
    <section aria-label="Your plan so far" className="ns-plan">
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 8 }}>
        <h3 style={{ margin: 0, fontSize: 22 }}>Your plan so far</h3>
        {!blank && (
          <span className="ns-mut" style={{ fontSize: 12 }}>
            {plural(accounts.length, "account")} · {plural(events.length, "event")}
          </span>
        )}
      </div>

      {blank ? (
        <div className="ns-pending ns-mut">
          A blank scenario starts with no accounts or events. Add them on the Portfolio and Plan tabs,
          or switch to Guided to answer a few questions first.
        </div>
      ) : (
        <>
          <Blueprint
            style={{
              padding: "16px 18px",
              display: "grid",
              gridTemplateColumns: "minmax(0, 200px) minmax(0, 1fr)",
              gap: 18,
              alignItems: "center",
            }}
          >
            <div>
              <StatLabel>Opening net worth</StatLabel>
              <div className="stat-v">{money(wealth)}</div>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ display: "flex", height: 8, gap: 2 }}>
                {funded.length === 0 ? (
                  <div style={{ flex: 1, background: "var(--color-divider)" }} />
                ) : (
                  funded.map((account, index) => (
                    <div
                      key={account.name}
                      style={{ flex: account.balance, background: FILLS[index % FILLS.length] }}
                    />
                  ))
                )}
              </div>
              <div className="ns-mut" style={{ display: "flex", flexWrap: "wrap", gap: "2px 14px", fontSize: 11.5 }}>
                {funded.length === 0
                  ? "No balances yet"
                  : funded.map((account) => (
                      <span key={account.name}>
                        {account.name} {money(account.balance)}
                      </span>
                    ))}
              </div>
            </div>
          </Blueprint>

          <div>
            <div style={{ marginBottom: 6 }}>
              <StatLabel>Accounts</StatLabel>
            </div>
            {accounts.length === 0 ? (
              <p className="ns-mut" style={{ margin: 0, fontSize: 13 }}>
                Your accounts appear here as you answer.
              </p>
            ) : (
              <div style={{ overflowX: "auto" }}>
                <Table style={{ fontSize: 13 }}>
                  <thead>
                    <tr>
                      <Th>Account</Th>
                      <Th>Tax</Th>
                      <Th>Return profile</Th>
                      <Th align="right">Balance</Th>
                    </tr>
                  </thead>
                  <tbody>
                    {accounts.map((account) => (
                      <tr key={account.name}>
                        <Td>
                          {account.name} <span className="ns-mut">{account.kind}</span>
                        </Td>
                        <Td>
                          <Tag tone={account.tax.tone}>{account.tax.label}</Tag>
                        </Td>
                        <Td>{account.profile}</Td>
                        <Td align="right">{money(account.balance)}</Td>
                      </tr>
                    ))}
                  </tbody>
                </Table>
              </div>
            )}
          </div>

          <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <StatLabel>Events</StatLabel>
            {events.map((event) => (
              <Blueprint key={event.name} className={event.fresh ? "ns-sentence fresh" : "ns-sentence"}>
                <span className="ns-sentence-name">{event.name}</span>
                {event.parts.map((part, index) => (
                  <Fragment key={index}>
                    {typeof part === "string" ? (
                      <span>{part}</span>
                    ) : "slot" in part ? (
                      <span className="ns-slot">{part.slot}</span>
                    ) : (
                      <span className="ns-slot x">{part.expr}</span>
                    )}
                  </Fragment>
                ))}
                {event.fresh && (
                  <span style={{ marginLeft: "auto" }}>
                    <Tag tone="outline">just added</Tag>
                  </span>
                )}
              </Blueprint>
            ))}
            {events.length === 0 && pending.length === 0 && (
              <p className="ns-mut" style={{ margin: 0, fontSize: 13 }}>
                No income or spending events. Add them later on the Plan tab.
              </p>
            )}
            {pending.map((line) => (
              <div key={line} className="ns-pending ns-mut">
                {line}
              </div>
            ))}
          </div>

          {assumptions && assumptions.length > 0 && (
            <div>
              <StatLabel>Assumptions</StatLabel>
              {assumptions.map((row) => (
                <div key={row.label} className="ns-crow">
                  <span style={{ flex: 1 }}>{row.label}</span>
                  <span className="ns-mut" style={{ textAlign: "right" }}>
                    {row.value}
                  </span>
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </section>
  );
}
