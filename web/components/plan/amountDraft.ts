import type { AmountSpec } from "@/lib/api/types";

export interface AmountNames {
  account: (id: number) => string;
  asset: (id: number) => string;
}

const quote = (name: string) => JSON.stringify(name);

/** The compiler's names are case-sensitive; quote anything outside its identifier grammar. */
export function parameterReference(name: string): string {
  return /^[_\p{L}][\p{L}\p{N}_]*$/u.test(name) ? `$${name}` : `$${quote(name)}`;
}

/** Rename a bound parameter token while leaving account strings and other text intact. */
export function renameParameterReferences(source: string, oldName: string, newName: string): string {
  return source.replace(/\$"(?:\\.|[^"\\])*"|\$[_\p{L}][\p{L}\p{N}_]*|"(?:\\.|[^"\\])*"/gu, (token) => {
    if (!token.startsWith("$")) return token;
    let name: string;
    try {
      name = token[1] === '"' ? JSON.parse(token.slice(1)) as string : token.slice(1);
    } catch {
      return token; // Keep an unfinished or malformed quoted token in the draft.
    }
    return name === oldName ? parameterReference(newName) : token;
  });
}

export function renameAmountReferences(amount: AmountSpec, oldName: string, newName: string): AmountSpec {
  switch (amount.kind) {
    case "Expression":
      return { ...amount, source: renameParameterReferences(amount.source, oldName, newName) };
    case "InflationAdjusted":
    case "Scale":
      return { ...amount, inner: renameAmountReferences(amount.inner, oldName, newName) };
    case "Min":
    case "Max":
    case "Sub":
    case "Add":
    case "Mul":
      return { ...amount,
        left: renameAmountReferences(amount.left, oldName, newName),
        right: renameAmountReferences(amount.right, oldName, newName) };
    default:
      return amount;
  }
}

export function amountSource(amount: AmountSpec, names: AmountNames): string {
  if (amount.kind === "Expression") return amount.source;
  const source = (nested: AmountSpec) => amountSource(nested, names);
  switch (amount.kind) {
    case "Fixed": return String(amount.value);
    case "InflationAdjusted": return `inflation(${source(amount.inner)})`;
    case "SourceBalance": return "source_balance()";
    case "ZeroTargetBalance": return "payoff()";
    case "TargetToBalance": return `top_up(${amount.value})`;
    case "AssetBalance": return `holding(${quote(names.account(amount.account_id))}, ${quote(names.asset(amount.asset_id))})`;
    case "AccountTotalBalance": return `balance(${quote(names.account(amount.account_id))})`;
    case "AccountCashBalance": return `cash(${quote(names.account(amount.account_id))})`;
    case "Min": return `min(${source(amount.left)}, ${source(amount.right)})`;
    case "Max": return `max(${source(amount.left)}, ${source(amount.right)})`;
    case "Sub": return `(${source(amount.left)} - ${source(amount.right)})`;
    case "Add": return `(${source(amount.left)} + ${source(amount.right)})`;
    case "Mul": return `(${source(amount.left)} * ${source(amount.right)})`;
    case "Scale": return `(${amount.factor} * ${source(amount.inner)})`;
    default: return exhaustive(amount);
  }
}

function exhaustive(value: never): never { throw new Error(`Unknown amount: ${String(value)}`); }

export function staticAmount(value: number, inflated: boolean): AmountSpec {
  return { kind: "Expression", source: inflated ? `inflation(${value})` : String(value) };
}

/** Only a literal, optionally in inflation(), is safe to show in the static field. */
export function readStaticAmount(amount: AmountSpec): { value: number; inflationAdjusted: boolean } | null {
  if (amount.kind === "Fixed") return { value: amount.value, inflationAdjusted: false };
  if (amount.kind === "InflationAdjusted" && amount.inner.kind === "Fixed") {
    return { value: amount.inner.value, inflationAdjusted: true };
  }
  if (amount.kind !== "Expression") return null;
  const text = amount.source.trim();
  const wrapper = /^inflation\(\s*([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)\s*\)$/.exec(text);
  const literal = /^([+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?)$/.exec(text);
  const match = wrapper ?? literal;
  if (!match) return null;
  const value = Number(match[1]);
  return Number.isFinite(value) ? { value, inflationAdjusted: !!wrapper } : null;
}

function rootAnnotation(source: string): { mode: "Gross" | "Net"; inner: string } | null {
  const text = source.trim();
  const match = /^(gross|net)\s*\(/.exec(text);
  if (!match) return null;
  const opening = match[0].length - 1;
  let depth = 0;
  let quoted = false;
  let escaped = false;
  for (let index = opening; index < text.length; index += 1) {
    const char = text[index];
    if (quoted) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quoted = false;
      continue;
    }
    if (char === '"') quoted = true;
    else if (char === "(") depth += 1;
    else if (char === ")") {
      depth -= 1;
      if (depth === 0 && index !== text.length - 1) return null;
      if (depth < 0) return null;
    }
  }
  if (quoted || depth !== 0) return null;
  return { mode: match[1] === "gross" ? "Gross" : "Net", inner: text.slice(opening + 1, -1) };
}

export function rootAmountMode(source: string): "Gross" | "Net" | null {
  return rootAnnotation(source)?.mode ?? null;
}

export function withRootAmountMode(source: string, mode: "Gross" | "Net"): string {
  const text = source.trim();
  const inner = rootAnnotation(text)?.inner ?? text;
  return `${mode.toLowerCase()}(${inner})`;
}

/** Convert a compiler byte span to browser UTF-16 offsets for highlighting. */
export function byteSpanToText(source: string, start: number, end: number): [number, number] {
  const bytes = new TextEncoder().encode(source);
  const prefix = new TextDecoder().decode(bytes.slice(0, Math.max(0, start)));
  const highlighted = new TextDecoder().decode(bytes.slice(Math.max(0, start), Math.max(start, end)));
  return [prefix.length, prefix.length + highlighted.length];
}

export interface TextRange { start: number; end: number }
export interface CompletionToken extends TextRange {
  query: string;
  kind: "parameter" | "quoted" | "identifier";
}

/** The full token at the cursor, including a quoted argument's closing quote. */
export function completionToken(source: string, position: number): CompletionToken | null {
  const cursor = Math.max(0, Math.min(position, source.length));
  let quotedFrom = -1;
  let escaped = false;
  for (let index = 0; index < cursor; index += 1) {
    const char = source[index];
    if (quotedFrom >= 0) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') quotedFrom = -1;
    } else if (char === '"') quotedFrom = index;
  }
  if (quotedFrom >= 0) {
    let close = cursor;
    let escaping = escaped;
    for (; close < source.length; close += 1) {
      const char = source[close];
      if (escaping) escaping = false;
      else if (char === "\\") escaping = true;
      else if (char === '"') break;
    }
    const parameter = quotedFrom > 0 && source[quotedFrom - 1] === "$";
    const start = parameter ? quotedFrom - 1 : quotedFrom;
    return {
      start,
      end: Math.min(close + 1, source.length),
      query: (parameter ? "$\"" : "") + source.slice(quotedFrom + 1, cursor),
      kind: parameter ? "parameter" : "quoted",
    };
  }
  const before = source.slice(0, cursor);
  const match = /(?:\$[\p{L}\p{N}_]*|[\p{L}_][\p{L}\p{N}_]*)$/u.exec(before);
  if (!match) return null;
  const start = cursor - match[0].length;
  const suffix = /^[\p{L}\p{N}_]*/u.exec(source.slice(cursor))?.[0].length ?? 0;
  return { start, end: cursor + suffix, query: match[0],
    kind: match[0].startsWith("$") ? "parameter" : "identifier" };
}

export function replaceText(source: string, range: TextRange, inserted: string): { source: string; cursor: number } {
  const start = Math.max(0, Math.min(range.start, source.length));
  const end = Math.max(start, Math.min(range.end, source.length));
  return { source: source.slice(0, start) + inserted + source.slice(end), cursor: start + inserted.length };
}

/** Pick an insertion range that keeps an existing quoted argument valid. */
export function nameInsertion(
  source: string,
  selection: TextRange,
  token: CompletionToken | null,
  name: string,
  kind: "parameter" | "account",
): { range: TextRange; text: string } {
  const encoded = kind === "parameter" ? parameterReference(name) : JSON.stringify(name);
  const matchesQuoted = token && (kind === "parameter"
    ? token.kind === "parameter" && source[token.start + 1] === '"'
    : token.kind === "quoted");
  if (!matchesQuoted) return { range: selection, text: encoded };
  const quoteStart = token.start + (source[token.start] === "$" ? 1 : 0);
  const quoteEnd = source[token.end - 1] === '"' ? token.end - 1 : token.end;
  if (selection.start !== selection.end) {
    return { range: selection, text: selection.start > quoteStart && selection.end <= quoteEnd
      ? JSON.stringify(name).slice(1, -1) : encoded };
  }
  return { range: token, text: encoded };
}
