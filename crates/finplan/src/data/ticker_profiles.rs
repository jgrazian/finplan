// Ticker-to-profile mapping database
// Maps well-known ETF/fund tickers to appropriate return profile suggestions

use super::profiles_data::ReturnProfileData;

/// A profile category with its associated tickers
pub struct ProfileCategory {
    pub profile_name: &'static str,
    pub profile_data: ReturnProfileData,
    pub tickers: &'static [&'static str],
}

/// Lookup result for a ticker
pub struct TickerMatch<'a> {
    pub profile_name: &'static str,
    pub profile_data: &'a ReturnProfileData,
}

/// All known profile categories - one entry per profile, many tickers each
/// Values sourced from finplan_core historical constants
pub const PROFILE_CATEGORIES: &[ProfileCategory] = &[
    // US Total Market - uses S&P 500 historical as proxy
    // Source: Robert Shiller, Yale University (1871-2024)
    ProfileCategory {
        profile_name: "US Total Market",
        profile_data: ReturnProfileData::Normal {
            mean: 0.11471,
            std_dev: 0.18146,
        },
        tickers: &["VTI", "VTSAX", "ITOT", "SPTM", "SCHB", "FSKAX", "FZROX"],
    },
    // S&P 500 Index
    // Source: Robert Shiller, Yale University (1871-2024)
    ProfileCategory {
        profile_name: "S&P 500",
        profile_data: ReturnProfileData::Normal {
            mean: 0.11471,
            std_dev: 0.18146,
        },
        tickers: &["VOO", "SPY", "IVV", "VFIAX", "FXAIX", "SWPPX"],
    },
    // US Small Cap
    // Source: Kenneth French Data Library (1926-2024)
    ProfileCategory {
        profile_name: "US Small Cap",
        profile_data: ReturnProfileData::Normal {
            mean: 0.147749,
            std_dev: 0.278003,
        },
        tickers: &["VB", "IJR", "SCHA", "VBR", "IWM", "VIOO", "VSMAX"],
    },
    // US Aggregate Bond
    // Source: Bloomberg US Aggregate Bond Index (2002-2024)
    ProfileCategory {
        profile_name: "US Aggregate Bond",
        profile_data: ReturnProfileData::Normal {
            mean: 0.0311818,
            std_dev: 0.0468972,
        },
        tickers: &["BND", "AGG", "VBTLX", "SCHZ", "FBND", "FXNAX"],
    },
    // International Developed Markets
    // Source: MSCI EAFE Index via EFA ETF (1991-2024)
    ProfileCategory {
        profile_name: "International Developed",
        profile_data: ReturnProfileData::Normal {
            mean: 0.0778324,
            std_dev: 0.188273,
        },
        tickers: &["VXUS", "VEA", "EFA", "IXUS", "IEFA", "SWISX", "FSPSX"],
    },
    // Emerging Markets
    // Source: MSCI Emerging Markets Index via EEM ETF (1992-2024)
    ProfileCategory {
        profile_name: "Emerging Markets",
        profile_data: ReturnProfileData::Normal {
            mean: 0.107264,
            std_dev: 0.347473,
        },
        tickers: &["VWO", "IEMG", "EEM", "SCHE", "VEMAX"],
    },
    // REITs (Real Estate Investment Trusts)
    // Source: FTSE NAREIT All Equity REITs Index via VNQ ETF (2003-2024)
    ProfileCategory {
        profile_name: "REITs",
        profile_data: ReturnProfileData::Normal {
            mean: 0.082752,
            std_dev: 0.195905,
        },
        tickers: &["VNQ", "IYR", "SCHH", "FREL", "VGSLX", "RWR"],
    },
    // Money Market / Short-Term Treasury
    // Source: US Treasury Bills (1933-2024)
    ProfileCategory {
        profile_name: "Money Market",
        profile_data: ReturnProfileData::Normal {
            mean: 0.0341782,
            std_dev: 0.0305423,
        },
        tickers: &["VGSH", "SHV", "BIL", "VMFXX", "SPAXX", "FDRXX", "SGOV"],
    },
    // Long-Term Treasury Bonds
    // Source: US Treasury Long-Term Bonds (1928-2024)
    ProfileCategory {
        profile_name: "Long-Term Treasury",
        profile_data: ReturnProfileData::Normal {
            mean: 0.047717,
            std_dev: 0.0700793,
        },
        tickers: &["TLT", "VGLT", "EDV", "SPTL", "ZROZ"],
    },
    // TIPS (Treasury Inflation-Protected Securities)
    // Source: Bloomberg US Treasury TIPS Index via TIP ETF (2002-2024)
    ProfileCategory {
        profile_name: "TIPS",
        profile_data: ReturnProfileData::Normal {
            mean: 0.0358924,
            std_dev: 0.0606518,
        },
        tickers: &["TIP", "VTIP", "SCHP", "STIP", "FIPDX"],
    },
    // Corporate Bonds
    // Source: Bloomberg US Corporate Bond Index via LQD ETF (2000-2024)
    ProfileCategory {
        profile_name: "US Corporate Bond",
        profile_data: ReturnProfileData::Normal {
            mean: 0.0441447,
            std_dev: 0.0697513,
        },
        tickers: &["LQD", "VCIT", "IGIB", "SPIB", "VCSH"],
    },
    // Gold
    // Source: London Bullion Market via GLD ETF (1999-2024)
    ProfileCategory {
        profile_name: "Gold",
        profile_data: ReturnProfileData::Normal {
            mean: 0.131744,
            std_dev: 0.173436,
        },
        tickers: &["GLD", "IAU", "SGOL", "GLDM"],
    },
];

/// Find the profile category for a given ticker (case-insensitive)
pub fn get_suggestion(ticker: &str) -> Option<TickerMatch<'_>> {
    let ticker_upper = ticker.to_uppercase();
    PROFILE_CATEGORIES
        .iter()
        .find(|cat| cat.tickers.iter().any(|t| *t == ticker_upper))
        .map(|cat| TickerMatch {
            profile_name: cat.profile_name,
            profile_data: &cat.profile_data,
        })
}

/// Check if a ticker has a known profile suggestion
pub fn is_known_ticker(ticker: &str) -> bool {
    get_suggestion(ticker).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_ticker_lookup() {
        assert!(get_suggestion("VTI").is_some());
        assert!(get_suggestion("vti").is_some()); // case insensitive
        assert!(get_suggestion("VOO").is_some());
        assert!(get_suggestion("BND").is_some());
    }

    #[test]
    fn test_unknown_ticker() {
        assert!(get_suggestion("UNKNOWN").is_none());
        assert!(get_suggestion("CUSTOM").is_none());
    }

    #[test]
    fn test_profile_name_mapping() {
        let vti = get_suggestion("VTI").unwrap();
        assert_eq!(vti.profile_name, "US Total Market");

        let bnd = get_suggestion("BND").unwrap();
        assert_eq!(bnd.profile_name, "US Aggregate Bond");

        let vwo = get_suggestion("VWO").unwrap();
        assert_eq!(vwo.profile_name, "Emerging Markets");
    }

    #[test]
    fn test_is_known_ticker() {
        assert!(is_known_ticker("VTI"));
        assert!(is_known_ticker("spy")); // case insensitive
        assert!(!is_known_ticker("CUSTOM_ASSET"));
    }
}
