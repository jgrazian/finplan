//! `find_return_profile`: which return assumption a holding gets.
//!
//! A ticker is classified into a broad class (US broad-market equity,
//! international equity, aggregate bonds, cash, ...) from a static table of
//! common funds, or from words in the fund's name. The class, not the fund, is
//! what a return profile describes: VTI, VOO and FXAIX all read as the same
//! S&P 500 history, so the answer is the user's own profile for that class
//! first, else the history preset the engine ships for it, as a
//! `new_return_profile` change to place once (one per class). Where no profile
//! and no preset fits a class (a target-date fund, crypto), the tool says so
//! and says what to do instead; it never invents a distribution.

use serde_json::{Value, json};

use crate::api::profiles::AssetClass;

/// One of the user's return profiles, as the drafting agent sees the library.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryProfile {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub asset_class: Option<AssetClass>,
}

/// The classes, with what each is called to a person and the history the
/// engine ships for it, if any.
struct ClassInfo {
    class: AssetClass,
    label: &'static str,
    /// `compile::HISTORY_PRESETS` id.
    preset: Option<&'static str>,
    /// The name of a profile created from the preset.
    profile_name: &'static str,
    /// What to do when no preset fits.
    fallback: &'static str,
    key: &'static str,
}

const CLASSES: &[ClassInfo] = &[
    ClassInfo {
        class: AssetClass::UsEquity,
        label: "US broad-market equity",
        preset: Some("sp500"),
        profile_name: "US Equity (S&P 500 history)",
        fallback: "",
        key: "class-us-equity",
    },
    ClassInfo {
        class: AssetClass::UsSmallCap,
        label: "US small-cap equity",
        preset: Some("us_small_cap"),
        profile_name: "US Small Cap (history)",
        fallback: "",
        key: "class-us-small-cap",
    },
    ClassInfo {
        class: AssetClass::GlobalEquity,
        label: "global equity",
        preset: None,
        profile_name: "Global Equity",
        fallback: "No shipped history covers global equity as one series. Model it as two holdings, about 60% US equity and 40% international equity (find_return_profile for each), or map it to US equity and say so in a check note.",
        key: "class-global-equity",
    },
    ClassInfo {
        class: AssetClass::IntlEquity,
        label: "international equity",
        preset: Some("intl_developed"),
        profile_name: "International Developed (history)",
        fallback: "",
        key: "class-intl-equity",
    },
    ClassInfo {
        class: AssetClass::Bonds,
        label: "aggregate bonds",
        preset: Some("us_agg_bonds"),
        profile_name: "US Aggregate Bonds (history)",
        fallback: "",
        key: "class-bonds",
    },
    ClassInfo {
        class: AssetClass::Reit,
        label: "real estate investment trusts",
        preset: Some("reits"),
        profile_name: "REITs (history)",
        fallback: "",
        key: "class-reit",
    },
    ClassInfo {
        class: AssetClass::Cash,
        label: "cash and money market",
        preset: Some("us_tbills"),
        profile_name: "Cash / T-Bills (history)",
        fallback: "",
        key: "class-cash",
    },
    ClassInfo {
        class: AssetClass::Commodity,
        label: "commodities",
        preset: None,
        profile_name: "Commodities",
        fallback: "Only gold has a shipped history (preset `gold`). For a gold fund use asset_class Commodity with `name` containing gold; for other commodities say in a check note that no history is modelled.",
        key: "class-commodity",
    },
    ClassInfo {
        class: AssetClass::Crypto,
        label: "crypto",
        preset: None,
        profile_name: "Crypto",
        fallback: "No history is shipped for crypto. Add the holding with no return profile (it keeps its price) and file a check note saying the growth is not modelled, or ask the user for an assumption.",
        key: "class-crypto",
    },
    ClassInfo {
        class: AssetClass::Balanced,
        label: "balanced or target-date",
        preset: None,
        profile_name: "Balanced",
        fallback: "A target-date or balanced fund is a mix, and no shipped history is. Split it into holdings by its glide path (for example 90% US equity and 10% bonds at 25 years out, about 60/40 near retirement) and say the split is an assumption in a check note, or ask the user which mix they want.",
        key: "class-balanced",
    },
];

fn info(class: AssetClass) -> &'static ClassInfo {
    CLASSES
        .iter()
        .find(|c| c.class == class)
        .expect("every class has an entry")
}

pub fn class_label(class: AssetClass) -> &'static str {
    info(class).label
}

/// The history preset that fits a class, when there is one.
pub fn preset_for(class: AssetClass) -> Option<&'static str> {
    info(class).preset
}

/// The `$new` key a created profile of this class is filed under.
pub fn class_key(class: AssetClass) -> &'static str {
    info(class).key
}

// ── the ticker table ────────────────────────────────────────────────────────

const US_EQUITY: &[&str] = &[
    "VTI", "VOO", "SPY", "IVV", "ITOT", "SCHB", "SCHX", "SPLG", "VV", "IWB", "IWV", "RSP", "DIA",
    "QQQ", "QQQM", "VUG", "VTV", "SCHG", "SCHD", "VIG", "VYM", "MGK", "SPYG", "SPYV", "IVW", "IWF",
    "FXAIX", "FSKAX", "FZROX", "FNILX", "FSPGX", "FCNTX", "SWTSX", "SWPPX", "VTSAX", "VFIAX",
    "VFINX", "VTSMX", "VIGAX", "VIVAX", "VIMAX", "VO", "IJH", "MDY",
];
const US_SMALL_CAP: &[&str] = &[
    "VB", "VBR", "VBK", "IJR", "IWM", "SCHA", "VTWO", "VSMAX", "NAESX", "SWSSX", "FSSNX", "VSIAX",
];
const INTL_EQUITY: &[&str] = &[
    "VXUS", "VEA", "IXUS", "VEU", "IEFA", "EFA", "SCHF", "SPDW", "VWO", "IEMG", "SCHE", "FTIHX",
    "FZILX", "SWISX", "VTIAX", "VTMGX", "VGTSX", "FSPSX", "VTPSX", "VWIGX",
];
const GLOBAL_EQUITY: &[&str] = &["VT", "ACWI", "VTWAX", "FTIGX", "SPGM", "URTH"];
const BONDS: &[&str] = &[
    "BND", "AGG", "FXNAX", "FXNAI", "FBND", "SCHZ", "VBTLX", "VBMFX", "SWAGX", "BIV", "BSV", "BLV",
    "IEF", "GOVT", "SHY", "VGSH", "VGIT", "VGLT", "TLT", "TIP", "VTIP", "SCHP", "VCIT", "VCSH",
    "LQD", "MUB", "VTEB", "FIPDX", "VIPSX", "VFICX", "PTTRX",
];
const REIT: &[&str] = &[
    "VNQ", "SCHH", "IYR", "XLRE", "RWR", "VGSLX", "VGSIX", "FRESX", "FSRNX", "VNQI",
];
const CASH: &[&str] = &[
    "VMFXX", "SPAXX", "FDRXX", "SWVXX", "FZFXX", "VMMXX", "SNSXX", "SGOV", "BIL", "SHV", "USFR",
    "TFLXX", "CASH", "FCASH", "CORE",
];
const COMMODITY: &[&str] = &["GLD", "IAU", "GLDM", "SGOL", "SLV", "PDBC", "DBC", "GSG"];
const CRYPTO: &[&str] = &[
    "IBIT", "FBTC", "GBTC", "BITO", "ETHA", "ETHE", "BTC", "ETH", "ARKB", "BITB",
];
const BALANCED: &[&str] = &[
    // Vanguard Target Retirement and LifeStrategy
    "VTTVX", "VTHRX", "VTTHX", "VFORX", "VTIVX", "VFIFX", "VFFVX", "VTWNX", "VTINX", "VLXVX",
    "VSVNX", "VTXVX", "VTTSX", "VSCGX", "VASGX", "VSMGX", "VASIX",
    // Fidelity Freedom, Schwab and T. Rowe Price target funds, balanced funds
    "FFFHX", "FFIZX", "FDEEX", "FDKLX", "FIPFX", "FXIFX", "SWYEX", "SWYRX", "TRRHX", "TRRNX",
    "VBIAX", "VWELX", "VBINX", "FBALX", "AOR", "AOM", "AOA", "AOK",
];

const TABLE: &[(&[&str], AssetClass)] = &[
    (US_EQUITY, AssetClass::UsEquity),
    (US_SMALL_CAP, AssetClass::UsSmallCap),
    (INTL_EQUITY, AssetClass::IntlEquity),
    (GLOBAL_EQUITY, AssetClass::GlobalEquity),
    (BONDS, AssetClass::Bonds),
    (REIT, AssetClass::Reit),
    (CASH, AssetClass::Cash),
    (COMMODITY, AssetClass::Commodity),
    (CRYPTO, AssetClass::Crypto),
    (BALANCED, AssetClass::Balanced),
];

/// The class of a listed ticker or mutual fund symbol, from the table of
/// common funds. Case and surrounding space do not matter.
pub fn classify_ticker(ticker: &str) -> Option<AssetClass> {
    let ticker: String = ticker
        .trim()
        .trim_start_matches('$')
        .to_ascii_uppercase()
        .replace(['.', ' '], "");
    if ticker.is_empty() {
        return None;
    }
    TABLE
        .iter()
        .find(|(tickers, _)| tickers.contains(&ticker.as_str()))
        .map(|(_, class)| *class)
}

/// The class a fund's name suggests: what a statement line such as "VANGUARD
/// TOTAL STOCK MKT INDEX ADM" says when its symbol is not in the table.
pub fn classify_name(name: &str) -> Option<AssetClass> {
    let name = name.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    // Most specific first: "target retirement 2050" is not "retirement".
    Some(
        if has(&[
            "target retirement",
            "target date",
            "target-date",
            "freedom 20",
            "freedom index 20",
            "lifepath",
            "lifestrategy",
            "lifecycle",
            "balanced",
            "asset allocation",
            "wellington",
        ]) {
            AssetClass::Balanced
        } else if has(&[
            "money market",
            "treasury bill",
            "t-bill",
            "cash reserves",
            "government cash",
            "settlement fund",
            "sweep",
        ]) {
            AssetClass::Cash
        } else if has(&[
            "bond",
            "aggregate",
            "fixed income",
            "treasury",
            "tips ",
            "income fund",
        ]) {
            AssetClass::Bonds
        } else if has(&["reit", "real estate"]) {
            AssetClass::Reit
        } else if has(&["gold", "precious metal", "commodit"]) {
            AssetClass::Commodity
        } else if has(&["bitcoin", "ethereum", "crypto"]) {
            AssetClass::Crypto
        } else if has(&["small cap", "small-cap", "smallcap", "russell 2000"]) {
            AssetClass::UsSmallCap
        } else if has(&[
            "emerging",
            "international",
            "intl",
            "ex-us",
            "ex us",
            "developed market",
            "foreign",
            "eafe",
            "europe",
            "pacific",
        ]) {
            AssetClass::IntlEquity
        } else if has(&[
            "all-world",
            "all world",
            "total world",
            "global",
            "world stock",
        ]) {
            AssetClass::GlobalEquity
        } else if has(&[
            "total stock",
            "total market",
            "s&p 500",
            "s&p500",
            "sp 500",
            "500 index",
            "large cap",
            "large-cap",
            "russell 1000",
            "russell 3000",
            "nasdaq",
            "dow jones",
            "extended market",
            "mid cap",
            "mid-cap",
            "growth",
            "dividend",
        ]) {
            AssetClass::UsEquity
        } else {
            return None;
        },
    )
}

/// A class named in words or in the profile column's own spelling:
/// `UsEquity`, `us_equity`, `US equity`, `bonds`, `intl`.
pub fn parse_class(text: &str) -> Option<AssetClass> {
    let squashed: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    Some(match squashed.as_str() {
        "usequity"
        | "usstocks"
        | "usstock"
        | "us"
        | "usbroad"
        | "usbroadmarket"
        | "usbroadmarketequity"
        | "domesticequity" => AssetClass::UsEquity,
        "ussmallcap" | "smallcap" => AssetClass::UsSmallCap,
        "globalequity" | "global" | "world" => AssetClass::GlobalEquity,
        "intlequity" | "internationalequity" | "international" | "intl" | "foreignequity" => {
            AssetClass::IntlEquity
        }
        "bonds" | "bond" | "aggregatebonds" | "fixedincome" => AssetClass::Bonds,
        "reit" | "reits" | "realestate" => AssetClass::Reit,
        "cash" | "moneymarket" | "tbills" | "cashequivalent" => AssetClass::Cash,
        "commodity" | "commodities" | "gold" => AssetClass::Commodity,
        "crypto" | "cryptocurrency" => AssetClass::Crypto,
        "balanced" | "targetdate" | "targetretirement" => AssetClass::Balanced,
        _ => return None,
    })
}

// ── the tool ────────────────────────────────────────────────────────────────

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "ticker": {"type": "string", "description": "A listed ticker or fund symbol, e.g. VTI, FXAIX, VMFXX."},
            "name": {"type": "string", "description": "The fund's name as a statement prints it; used when the ticker is missing or not in the table."},
            "asset_class": {
                "type": "string",
                "enum": ["UsEquity", "UsSmallCap", "GlobalEquity", "IntlEquity", "Bonds", "Reit", "Cash", "Commodity", "Crypto", "Balanced"],
                "description": "The class itself, when you already know it."
            }
        }
    })
}

/// How the class was found, for the reply.
fn matched(input: &Value) -> Result<(AssetClass, String), String> {
    let text = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|t| !t.is_empty())
    };
    if let Some(class) = text("asset_class") {
        return parse_class(class)
            .map(|c| (c, format!("asset_class {class}")))
            .ok_or_else(|| {
                format!(
                    "`{class}` is not an asset class; use one of UsEquity, UsSmallCap, GlobalEquity, IntlEquity, Bonds, Reit, Cash, Commodity, Crypto, Balanced"
                )
            });
    }
    if let Some(ticker) = text("ticker")
        && let Some(class) = classify_ticker(ticker)
    {
        return Ok((class, format!("ticker {}", ticker.to_ascii_uppercase())));
    }
    if let Some(name) = text("name")
        && let Some(class) = classify_name(name)
    {
        return Ok((class, format!("the name \"{name}\"")));
    }
    match (text("ticker"), text("name")) {
        (None, None) => Err("give a ticker, a name or an asset_class".into()),
        (ticker, name) => Err(format!(
            "{} is not in the table of common funds and its name says nothing certain about its class. Ask the user, or pass asset_class if you know it, and say in a check note which assumption you used",
            ticker
                .map(|t| format!("`{}`", t.to_ascii_uppercase()))
                .or(name.map(|n| format!("\"{n}\"")))
                .unwrap_or_default()
        )),
    }
}

/// `find_return_profile`: the user's own profile for the class, else the
/// history preset for it as a `new_return_profile` change, else what to do.
pub fn run(input: &Value, library: &[LibraryProfile]) -> Result<Value, String> {
    let (class, how) = matched(input)?;
    let entry = info(class);
    let mut answer = json!({
        "asset_class": entry.class,
        "label": entry.label,
        "matched_by": how,
    });

    let existing: Vec<&LibraryProfile> = library
        .iter()
        .filter(|p| p.asset_class == Some(class))
        .collect();
    if let Some(profile) = existing.first() {
        answer["source"] = json!("existing_profile");
        answer["profile"] = json!({
            "id": profile.id,
            "name": profile.name,
            "description": profile.description,
        });
        answer["other_profiles_of_class"] = json!(existing.len() - 1);
        answer["how_to_use"] = json!(format!(
            "Set the asset's return_profile_id to {}. Every fund of this class uses this one profile.",
            profile.id
        ));
        return Ok(answer);
    }

    if let Some(preset) = entry.preset {
        answer["source"] = json!("history_preset");
        answer["preset"] = json!(preset);
        answer["change"] = json!({
            "op": "add",
            "target": {"new_return_profile": entry.key},
            "path": "",
            "value": {
                "name": entry.profile_name,
                "description": format!("Resampled annual returns of the {preset} history the engine ships"),
                "asset_class": entry.class,
                "distribution": {"kind": "Bootstrap", "preset": preset},
            },
        });
        answer["how_to_use"] = json!(format!(
            "The user has no {} profile. Put this change in the step that creates the assets and set each asset's return_profile_id to {{\"$new\": \"{}\"}}. One profile serves every fund of the class: reuse the key, never add a second. Once it is in the plan, this tool returns it as an existing profile.",
            entry.label, entry.key
        ));
        return Ok(answer);
    }

    answer["source"] = json!("none");
    answer["how_to_use"] = json!(entry.fallback);
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> Vec<LibraryProfile> {
        vec![
            LibraryProfile {
                id: 11,
                name: "US Total Market".into(),
                description: Some("S&P 500, 1928-2024".into()),
                asset_class: Some(AssetClass::UsEquity),
            },
            LibraryProfile {
                id: 12,
                name: "Savings Account".into(),
                description: None,
                asset_class: None,
            },
        ]
    }

    #[test]
    fn common_funds_classify_by_ticker() {
        for (ticker, class) in [
            ("VTI", AssetClass::UsEquity),
            ("VOO", AssetClass::UsEquity),
            ("SPY", AssetClass::UsEquity),
            ("FXAIX", AssetClass::UsEquity),
            ("SWTSX", AssetClass::UsEquity),
            ("VXUS", AssetClass::IntlEquity),
            ("VEA", AssetClass::IntlEquity),
            ("IXUS", AssetClass::IntlEquity),
            ("BND", AssetClass::Bonds),
            ("AGG", AssetClass::Bonds),
            ("FXNAI", AssetClass::Bonds),
            ("VMFXX", AssetClass::Cash),
            ("SPAXX", AssetClass::Cash),
            ("VNQ", AssetClass::Reit),
            ("IWM", AssetClass::UsSmallCap),
            ("VT", AssetClass::GlobalEquity),
            ("GLD", AssetClass::Commodity),
            ("IBIT", AssetClass::Crypto),
            ("VTTVX", AssetClass::Balanced),
            ("FFFHX", AssetClass::Balanced),
        ] {
            assert_eq!(classify_ticker(ticker), Some(class), "{ticker}");
        }
        // Case, space and a leading $ are not part of the symbol.
        assert_eq!(classify_ticker(" vti "), Some(AssetClass::UsEquity));
        assert_eq!(classify_ticker("$VOO"), Some(AssetClass::UsEquity));
        assert_eq!(classify_ticker("ZZZZZ"), None);
        assert_eq!(classify_ticker(""), None);
    }

    #[test]
    fn no_ticker_sits_in_two_classes() {
        let mut seen = std::collections::HashSet::new();
        for (tickers, _) in TABLE {
            for ticker in *tickers {
                assert!(seen.insert(*ticker), "{ticker} is in two classes");
            }
        }
    }

    #[test]
    fn a_fund_name_classifies_when_the_symbol_is_unknown() {
        for (name, class) in [
            (
                "Vanguard Total Stock Market Index Adm",
                AssetClass::UsEquity,
            ),
            ("FIDELITY 500 INDEX FUND", AssetClass::UsEquity),
            (
                "Vanguard Total International Stock Index",
                AssetClass::IntlEquity,
            ),
            ("Vanguard Target Retirement 2050", AssetClass::Balanced),
            ("Fidelity Government Cash Reserves", AssetClass::Cash),
            ("Vanguard Total Bond Market Index", AssetClass::Bonds),
            ("Schwab US Small-Cap ETF", AssetClass::UsSmallCap),
            ("Vanguard Real Estate Index", AssetClass::Reit),
            ("iShares Gold Trust", AssetClass::Commodity),
            ("Vanguard Total World Stock", AssetClass::GlobalEquity),
        ] {
            assert_eq!(classify_name(name), Some(class), "{name}");
        }
        assert_eq!(classify_name("Acme Holdings Inc"), None);
    }

    #[test]
    fn classes_parse_from_words_and_from_the_column_spelling() {
        assert_eq!(parse_class("UsEquity"), Some(AssetClass::UsEquity));
        assert_eq!(parse_class("us_equity"), Some(AssetClass::UsEquity));
        assert_eq!(parse_class("Intl equity"), Some(AssetClass::IntlEquity));
        assert_eq!(parse_class("bonds"), Some(AssetClass::Bonds));
        assert_eq!(parse_class("lottery tickets"), None);
    }

    #[test]
    fn the_users_own_profile_for_the_class_comes_first() {
        let answer = run(&json!({"ticker": "VOO"}), &library()).unwrap();
        assert_eq!(answer["source"], "existing_profile");
        assert_eq!(answer["profile"]["id"], 11);
        assert_eq!(answer["asset_class"], "UsEquity");
        assert!(answer.get("change").is_none());
        // Every fund of the class gets the same answer.
        let other = run(&json!({"ticker": "FXAIX"}), &library()).unwrap();
        assert_eq!(other["profile"]["id"], 11);
    }

    #[test]
    fn with_no_profile_for_the_class_the_history_preset_is_offered_once_per_class() {
        let answer = run(&json!({"ticker": "VXUS"}), &library()).unwrap();
        assert_eq!(answer["source"], "history_preset");
        assert_eq!(answer["preset"], "intl_developed");
        let change = &answer["change"];
        assert_eq!(change["op"], "add");
        assert_eq!(change["target"]["new_return_profile"], "class-intl-equity");
        assert_eq!(change["value"]["distribution"]["kind"], "Bootstrap");
        assert_eq!(change["value"]["distribution"]["preset"], "intl_developed");
        assert_eq!(change["value"]["asset_class"], "IntlEquity");
        // A second fund of the class gets the same key, not a second profile.
        let second = run(&json!({"ticker": "VEA"}), &library()).unwrap();
        assert_eq!(second["change"]["target"], change["target"]);

        let cash = run(&json!({"ticker": "SPAXX"}), &[]).unwrap();
        assert_eq!(cash["preset"], "us_tbills");
        let bonds = run(&json!({"ticker": "BND"}), &[]).unwrap();
        assert_eq!(bonds["preset"], "us_agg_bonds");
    }

    #[test]
    fn a_class_no_preset_fits_gets_advice_and_no_profile() {
        for ticker in ["VTTVX", "IBIT", "VT"] {
            let answer = run(&json!({"ticker": ticker}), &library()).unwrap();
            assert_eq!(answer["source"], "none", "{ticker}");
            assert!(answer.get("change").is_none());
            assert!(!answer["how_to_use"].as_str().unwrap().is_empty());
        }
    }

    #[test]
    fn an_unknown_fund_says_what_to_do() {
        let error = run(&json!({"ticker": "ZZZZZ"}), &library()).unwrap_err();
        assert!(error.contains("ZZZZZ"));
        assert!(run(&json!({}), &library()).is_err());
        assert!(run(&json!({"asset_class": "lottery"}), &library()).is_err());
        let by_class = run(&json!({"asset_class": "bonds"}), &[]).unwrap();
        assert_eq!(by_class["preset"], "us_agg_bonds");
        let by_name = run(
            &json!({"name": "Vanguard Total Stock Market Index"}),
            &library(),
        )
        .unwrap();
        assert_eq!(by_name["profile"]["id"], 11);
    }
}
