/**
 * What a ticker is, and what it should therefore grow by.
 *
 * An asset is a name, a price and a return profile, and two of the three can be
 * inferred from the first: `VTI` is the Vanguard Total Stock Market ETF, and a
 * total-market fund belongs on whatever the library calls US equities. Typing
 * the ticker is the part only the user can do; the rest is filled in.
 *
 * The table is bundled rather than fetched. A plan is edited offline as often
 * as not, a symbol lookup is a paid API in every form worth having, and the
 * consequence of a miss here is a blank name — not a wrong number. Anything
 * unrecognised falls through untouched: no name, no mapping, and the asset is
 * created exactly as it was before this existed.
 *
 * The mapping to a profile goes through an asset class rather than naming a
 * profile directly, because the library is the user's own — seeded at
 * registration, then renamed, edited and deleted at will. The class is a
 * *stored* field on the profile (`asset_class`), not a guess read back off its
 * name: a library with nothing classed as bonds simply maps no bond fund, and
 * renaming "US Total Market" moves nothing.
 */
import type { AssetClass, Profile } from "@/lib/api/types";

/** How a class describes itself in a one-line note under the ticker field. */
export const CLASS_LABEL: Record<AssetClass, string> = {
  UsEquity: "US equity",
  UsSmallCap: "US small cap",
  GlobalEquity: "Global equity",
  IntlEquity: "International equity",
  Bonds: "Bonds",
  Reit: "Real estate",
  Cash: "Cash",
  Commodity: "Commodity",
  Crypto: "Crypto",
  Balanced: "Balanced",
};

/**
 * Every class, in the order a picker lists them. Read off the label table so a
 * variant added to the Rust enum cannot be offered here without a label — the
 * `Record` is what fails to compile until one is written.
 */
export const ASSET_CLASSES = Object.keys(CLASS_LABEL) as AssetClass[];

/**
 * Where a class has no profile of its own, the next best one to stand in.
 *
 * A small-cap fund on a broad US equity assumption is close; on flat zero it is
 * not close at all, and flat zero is what an unmapped asset gets. Nothing falls
 * back to a class of a different shape — a bond fund never lands on equities.
 */
const FALLBACK: Partial<Record<AssetClass, AssetClass>> = {
  UsSmallCap: "UsEquity",
  GlobalEquity: "UsEquity",
  IntlEquity: "GlobalEquity",
};

export interface TickerInfo {
  /** The fund or company the symbol names. */
  name: string;
  assetClass: AssetClass;
}

/**
 * Ticker → what it is. Deliberately shallow and wide: the funds a retirement
 * plan is actually built out of, plus the large caps people hold single
 * positions in. A stock maps to the broad US market, which is the right
 * *return* assumption and the wrong *risk* one — the note beside the field says
 * which profile was chosen, so an assumption is never silently applied.
 */
const TICKERS: Record<string, TickerInfo> = Object.fromEntries(
  (
    [
      // ── US broad market ─────────────────────────────────────────────────
      ["VTI", "Vanguard Total Stock Market ETF", "UsEquity"],
      ["VOO", "Vanguard S&P 500 ETF", "UsEquity"],
      ["SPY", "SPDR S&P 500 ETF Trust", "UsEquity"],
      ["IVV", "iShares Core S&P 500 ETF", "UsEquity"],
      ["ITOT", "iShares Core S&P Total US Stock Market ETF", "UsEquity"],
      ["SCHB", "Schwab US Broad Market ETF", "UsEquity"],
      ["SCHX", "Schwab US Large-Cap ETF", "UsEquity"],
      ["VV", "Vanguard Large-Cap ETF", "UsEquity"],
      ["VUG", "Vanguard Growth ETF", "UsEquity"],
      ["VTV", "Vanguard Value ETF", "UsEquity"],
      ["VYM", "Vanguard High Dividend Yield ETF", "UsEquity"],
      ["SCHD", "Schwab US Dividend Equity ETF", "UsEquity"],
      ["QQQ", "Invesco QQQ Trust", "UsEquity"],
      ["QQQM", "Invesco NASDAQ 100 ETF", "UsEquity"],
      ["DIA", "SPDR Dow Jones Industrial Average ETF Trust", "UsEquity"],
      ["VO", "Vanguard Mid-Cap ETF", "UsEquity"],
      ["IJH", "iShares Core S&P Mid-Cap ETF", "UsEquity"],
      // Index funds, which is what a 401(k) menu is made of.
      ["VTSAX", "Vanguard Total Stock Market Index Fund Admiral Shares", "UsEquity"],
      ["VFIAX", "Vanguard 500 Index Fund Admiral Shares", "UsEquity"],
      ["FXAIX", "Fidelity 500 Index Fund", "UsEquity"],
      ["FSKAX", "Fidelity Total Market Index Fund", "UsEquity"],
      ["FZROX", "Fidelity ZERO Total Market Index Fund", "UsEquity"],
      ["SWPPX", "Schwab S&P 500 Index Fund", "UsEquity"],
      ["SWTSX", "Schwab Total Stock Market Index Fund", "UsEquity"],

      // ── US small cap ────────────────────────────────────────────────────
      ["VB", "Vanguard Small-Cap ETF", "UsSmallCap"],
      ["VBR", "Vanguard Small-Cap Value ETF", "UsSmallCap"],
      ["VXF", "Vanguard Extended Market ETF", "UsSmallCap"],
      ["IJR", "iShares Core S&P Small-Cap ETF", "UsSmallCap"],
      ["IWM", "iShares Russell 2000 ETF", "UsSmallCap"],
      ["SCHA", "Schwab US Small-Cap ETF", "UsSmallCap"],
      ["VSMAX", "Vanguard Small-Cap Index Fund Admiral Shares", "UsSmallCap"],
      ["FSSNX", "Fidelity Small Cap Index Fund", "UsSmallCap"],

      // ── Global and international ────────────────────────────────────────
      ["VT", "Vanguard Total World Stock ETF", "GlobalEquity"],
      ["ACWI", "iShares MSCI ACWI ETF", "GlobalEquity"],
      ["VTWAX", "Vanguard Total World Stock Index Fund Admiral Shares", "GlobalEquity"],
      ["VXUS", "Vanguard Total International Stock ETF", "IntlEquity"],
      ["VEA", "Vanguard FTSE Developed Markets ETF", "IntlEquity"],
      ["VEU", "Vanguard FTSE All-World ex-US ETF", "IntlEquity"],
      ["VWO", "Vanguard FTSE Emerging Markets ETF", "IntlEquity"],
      ["VGK", "Vanguard FTSE Europe ETF", "IntlEquity"],
      ["VPL", "Vanguard FTSE Pacific ETF", "IntlEquity"],
      ["IEFA", "iShares Core MSCI EAFE ETF", "IntlEquity"],
      ["EFA", "iShares MSCI EAFE ETF", "IntlEquity"],
      ["IEMG", "iShares Core MSCI Emerging Markets ETF", "IntlEquity"],
      ["SCHF", "Schwab International Equity ETF", "IntlEquity"],
      ["VTIAX", "Vanguard Total International Stock Index Fund Admiral Shares", "IntlEquity"],
      ["FTIHX", "Fidelity Total International Index Fund", "IntlEquity"],

      // ── Bonds ───────────────────────────────────────────────────────────
      ["BND", "Vanguard Total Bond Market ETF", "Bonds"],
      ["BNDX", "Vanguard Total International Bond ETF", "Bonds"],
      ["AGG", "iShares Core US Aggregate Bond ETF", "Bonds"],
      ["SCHZ", "Schwab US Aggregate Bond ETF", "Bonds"],
      ["TLT", "iShares 20+ Year Treasury Bond ETF", "Bonds"],
      ["IEF", "iShares 7-10 Year Treasury Bond ETF", "Bonds"],
      ["SHY", "iShares 1-3 Year Treasury Bond ETF", "Bonds"],
      ["LQD", "iShares iBoxx Investment Grade Corporate Bond ETF", "Bonds"],
      ["HYG", "iShares iBoxx High Yield Corporate Bond ETF", "Bonds"],
      ["MUB", "iShares National Muni Bond ETF", "Bonds"],
      ["TIP", "iShares TIPS Bond ETF", "Bonds"],
      ["VTEB", "Vanguard Tax-Exempt Bond ETF", "Bonds"],
      ["VCIT", "Vanguard Intermediate-Term Corporate Bond ETF", "Bonds"],
      ["VBTLX", "Vanguard Total Bond Market Index Fund Admiral Shares", "Bonds"],
      ["FXNAX", "Fidelity US Bond Index Fund", "Bonds"],

      // ── Real estate ─────────────────────────────────────────────────────
      ["VNQ", "Vanguard Real Estate ETF", "Reit"],
      ["VGSLX", "Vanguard Real Estate Index Fund Admiral Shares", "Reit"],
      ["SCHH", "Schwab US REIT ETF", "Reit"],
      ["IYR", "iShares US Real Estate ETF", "Reit"],
      ["XLRE", "Real Estate Select Sector SPDR Fund", "Reit"],
      ["O", "Realty Income Corporation", "Reit"],

      // ── Cash and equivalents ────────────────────────────────────────────
      ["VMFXX", "Vanguard Federal Money Market Fund", "Cash"],
      ["VUSXX", "Vanguard Treasury Money Market Fund", "Cash"],
      ["SPAXX", "Fidelity Government Money Market Fund", "Cash"],
      ["FDRXX", "Fidelity Government Cash Reserves", "Cash"],
      ["SWVXX", "Schwab Value Advantage Money Fund", "Cash"],
      ["SGOV", "iShares 0-3 Month Treasury Bond ETF", "Cash"],
      ["BIL", "SPDR Bloomberg 1-3 Month T-Bill ETF", "Cash"],

      // ── Commodities and crypto ──────────────────────────────────────────
      ["GLD", "SPDR Gold Shares", "Commodity"],
      ["GLDM", "SPDR Gold MiniShares Trust", "Commodity"],
      ["IAU", "iShares Gold Trust", "Commodity"],
      ["SLV", "iShares Silver Trust", "Commodity"],
      ["IBIT", "iShares Bitcoin Trust ETF", "Crypto"],
      ["FBTC", "Fidelity Wise Origin Bitcoin Fund", "Crypto"],
      ["GBTC", "Grayscale Bitcoin Trust ETF", "Crypto"],

      // ── Target-date and balanced ────────────────────────────────────────
      ["VBIAX", "Vanguard Balanced Index Fund Admiral Shares", "Balanced"],
      ["VFIFX", "Vanguard Target Retirement 2050 Fund", "Balanced"],
      ["VFFVX", "Vanguard Target Retirement 2055 Fund", "Balanced"],
      ["VTTHX", "Vanguard Target Retirement 2035 Fund", "Balanced"],
      ["VTIVX", "Vanguard Target Retirement 2045 Fund", "Balanced"],

      // ── Single names ────────────────────────────────────────────────────
      ["AAPL", "Apple Inc.", "UsEquity"],
      ["MSFT", "Microsoft Corporation", "UsEquity"],
      ["GOOGL", "Alphabet Inc. Class A", "UsEquity"],
      ["GOOG", "Alphabet Inc. Class C", "UsEquity"],
      ["AMZN", "Amazon.com, Inc.", "UsEquity"],
      ["NVDA", "NVIDIA Corporation", "UsEquity"],
      ["META", "Meta Platforms, Inc.", "UsEquity"],
      ["TSLA", "Tesla, Inc.", "UsEquity"],
      ["BRKB", "Berkshire Hathaway Inc. Class B", "UsEquity"],
      ["BRKA", "Berkshire Hathaway Inc. Class A", "UsEquity"],
      ["AVGO", "Broadcom Inc.", "UsEquity"],
      ["LLY", "Eli Lilly and Company", "UsEquity"],
      ["JPM", "JPMorgan Chase & Co.", "UsEquity"],
      ["V", "Visa Inc.", "UsEquity"],
      ["MA", "Mastercard Incorporated", "UsEquity"],
      ["UNH", "UnitedHealth Group Incorporated", "UsEquity"],
      ["JNJ", "Johnson & Johnson", "UsEquity"],
      ["XOM", "Exxon Mobil Corporation", "UsEquity"],
      ["CVX", "Chevron Corporation", "UsEquity"],
      ["PG", "The Procter & Gamble Company", "UsEquity"],
      ["KO", "The Coca-Cola Company", "UsEquity"],
      ["PEP", "PepsiCo, Inc.", "UsEquity"],
      ["WMT", "Walmart Inc.", "UsEquity"],
      ["COST", "Costco Wholesale Corporation", "UsEquity"],
      ["HD", "The Home Depot, Inc.", "UsEquity"],
      ["MCD", "McDonald's Corporation", "UsEquity"],
      ["NKE", "NIKE, Inc.", "UsEquity"],
      ["SBUX", "Starbucks Corporation", "UsEquity"],
      ["DIS", "The Walt Disney Company", "UsEquity"],
      ["NFLX", "Netflix, Inc.", "UsEquity"],
      ["CRM", "Salesforce, Inc.", "UsEquity"],
      ["ORCL", "Oracle Corporation", "UsEquity"],
      ["ADBE", "Adobe Inc.", "UsEquity"],
      ["IBM", "International Business Machines Corporation", "UsEquity"],
      ["CSCO", "Cisco Systems, Inc.", "UsEquity"],
      ["INTC", "Intel Corporation", "UsEquity"],
      ["AMD", "Advanced Micro Devices, Inc.", "UsEquity"],
      ["QCOM", "QUALCOMM Incorporated", "UsEquity"],
      ["TXN", "Texas Instruments Incorporated", "UsEquity"],
      ["MRK", "Merck & Co., Inc.", "UsEquity"],
      ["ABBV", "AbbVie Inc.", "UsEquity"],
      ["PFE", "Pfizer Inc.", "UsEquity"],
      ["BAC", "Bank of America Corporation", "UsEquity"],
      ["WFC", "Wells Fargo & Company", "UsEquity"],
      ["GS", "The Goldman Sachs Group, Inc.", "UsEquity"],
      ["T", "AT&T Inc.", "UsEquity"],
      ["VZ", "Verizon Communications Inc.", "UsEquity"],
      ["BA", "The Boeing Company", "UsEquity"],
      ["CAT", "Caterpillar Inc.", "UsEquity"],
      ["GE", "GE Aerospace", "UsEquity"],
      ["F", "Ford Motor Company", "UsEquity"],
      ["GM", "General Motors Company", "UsEquity"],
      ["UBER", "Uber Technologies, Inc.", "UsEquity"],
      ["PYPL", "PayPal Holdings, Inc.", "UsEquity"],
      ["PLTR", "Palantir Technologies Inc.", "UsEquity"],
    ] as ReadonlyArray<readonly [string, string, AssetClass]>
  ).map(([symbol, name, assetClass]) => [symbol, { name, assetClass }]),
);

/**
 * `brk.b`, `BRK-B` and ` brk b ` are one symbol. Punctuation is how a class
 * share is written, and every writing of it means the same holding.
 */
function normalize(ticker: string): string {
  return ticker.trim().toUpperCase().replace(/[^A-Z0-9]/g, "");
}

/** What the table knows about a symbol, or nothing if it does not know it. */
export function lookupTicker(ticker: string): TickerInfo | undefined {
  const symbol = normalize(ticker);
  return symbol === "" ? undefined : TICKERS[symbol];
}

/**
 * The profile in *this* library that a class belongs on, or none where the
 * library has nothing describing it — a gold ETF in a library of stock and bond
 * assumptions, say, which is better left unmapped than mapped to equities.
 *
 * Two profiles may claim one class (an optimistic and a pessimistic equity
 * assumption, both kept); the first the server listed wins, which is the
 * alphabetically first, and either is a defensible prefill the user can change.
 */
export function profileForClass(
  assetClass: AssetClass,
  profiles: Profile[],
): Profile | undefined {
  // The walk is bounded by what it has already tried, so a fallback chain that
  // is ever made circular cannot hang the field it is typed into.
  const tried = new Set<AssetClass>();
  let target: AssetClass | undefined = assetClass;
  while (target !== undefined && !tried.has(target)) {
    const current: AssetClass = target;
    tried.add(current);
    const hit = profiles.find((p) => p.asset_class === current);
    if (hit) return hit;
    target = FALLBACK[current];
  }
  return undefined;
}

/** Everything a form can prefill from a ticker, resolved against a library. */
export interface TickerDefaults extends TickerInfo {
  classLabel: string;
  /** The profile the class resolved to; absent where the library has none. */
  profile?: Profile;
}

export function tickerDefaults(
  ticker: string,
  profiles: Profile[],
): TickerDefaults | undefined {
  const info = lookupTicker(ticker);
  if (!info) return undefined;
  return {
    ...info,
    classLabel: CLASS_LABEL[info.assetClass],
    profile: profileForClass(info.assetClass, profiles),
  };
}
