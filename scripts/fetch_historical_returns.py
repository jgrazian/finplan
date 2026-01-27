#!/usr/bin/env python3
"""
Fetch historical return data from reputable academic and financial sources
and compute statistics for finplan return profile presets.

Data Sources (in order of preference):
1. Robert Shiller's Online Data (Yale) - S&P 500 back to 1871
2. Kenneth French Data Library (Dartmouth) - Fama-French factors, market returns
3. Aswath Damodaran (NYU Stern) - Comprehensive asset class returns
4. FRED (Federal Reserve) - T-bills, inflation, bond yields
5. Yahoo Finance - Recent ETF data as fallback

Output: Rust const definitions for ReturnProfile presets
"""

import argparse
import hashlib
import io
import json
import os
import pickle
import zipfile
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path
from typing import Optional
from urllib.request import urlopen, Request
from urllib.error import URLError

import numpy as np
import pandas as pd


# ============================================================================
# Environment Loading
# ============================================================================

def load_env_file(env_path: Path) -> dict[str, str]:
    """Load environment variables from a .env file."""
    env_vars = {}
    if not env_path.exists():
        return env_vars

    with open(env_path) as f:
        for line in f:
            line = line.strip()
            # Skip comments and empty lines
            if not line or line.startswith('#'):
                continue
            # Parse KEY=VALUE (handle quoted values)
            if '=' in line:
                key, _, value = line.partition('=')
                key = key.strip()
                value = value.strip()
                # Remove quotes if present
                if (value.startswith('"') and value.endswith('"')) or \
                   (value.startswith("'") and value.endswith("'")):
                    value = value[1:-1]
                env_vars[key] = value
    return env_vars


def load_dotenv():
    """Load .env file from script directory or repo root."""
    script_dir = Path(__file__).parent
    repo_root = script_dir.parent

    # Try multiple locations
    env_files = [
        script_dir / '.env',
        repo_root / '.env',
        script_dir / '.env.local',
        repo_root / '.env.local',
    ]

    for env_file in env_files:
        if env_file.exists():
            env_vars = load_env_file(env_file)
            if env_vars:
                print(f"Loaded environment from {env_file}")
                # Set environment variables (don't override existing)
                for key, value in env_vars.items():
                    if key not in os.environ:
                        os.environ[key] = value
                return True
    return False

# ============================================================================
# Caching
# ============================================================================

CACHE_DIR = Path(__file__).parent / ".data_cache"
CACHE_MAX_AGE_DAYS = 30  # Re-download data older than this


def get_cache_path(url: str, suffix: str = ".pkl") -> Path:
    """Get cache file path for a URL."""
    url_hash = hashlib.md5(url.encode()).hexdigest()[:12]
    return CACHE_DIR / f"{url_hash}{suffix}"


def is_cache_valid(cache_path: Path, max_age_days: int = CACHE_MAX_AGE_DAYS) -> bool:
    """Check if cache file exists and is not too old."""
    if not cache_path.exists():
        return False
    age = datetime.now().timestamp() - cache_path.stat().st_mtime
    return age < max_age_days * 86400


def save_to_cache(data: bytes, url: str, suffix: str = ".bin") -> None:
    """Save raw data to cache."""
    CACHE_DIR.mkdir(exist_ok=True)
    cache_path = get_cache_path(url, suffix)
    cache_path.write_bytes(data)
    print(f"    [Cached to {cache_path.name}]")


def load_from_cache(url: str, suffix: str = ".bin") -> Optional[bytes]:
    """Load raw data from cache if valid."""
    cache_path = get_cache_path(url, suffix)
    if is_cache_valid(cache_path):
        print(f"    [Using cached {cache_path.name}]")
        return cache_path.read_bytes()
    return None


def fetch_url_cached(url: str, suffix: str = ".bin") -> bytes:
    """Fetch URL with caching."""
    cached = load_from_cache(url, suffix)
    if cached is not None:
        return cached

    print(f"    [Downloading from {url[:50]}...]")
    req = Request(url, headers={'User-Agent': 'Mozilla/5.0'})
    with urlopen(req, timeout=60) as response:
        data = response.read()

    save_to_cache(data, url, suffix)
    return data

# Optional imports with graceful degradation
try:
    import yfinance as yf
    HAS_YFINANCE = True
except ImportError:
    HAS_YFINANCE = False
    print("Warning: yfinance not installed. Run: pip install yfinance")

try:
    from fredapi import Fred
    HAS_FRED = True
except ImportError:
    HAS_FRED = False
    print("Warning: fredapi not installed. Run: pip install fredapi")

try:
    import openpyxl
    HAS_OPENPYXL = True
except ImportError:
    HAS_OPENPYXL = False
    print("Warning: openpyxl not installed. Run: pip install openpyxl (needed for Shiller data)")


# ============================================================================
# Data Classes
# ============================================================================

@dataclass
class AssetStats:
    """Statistics for an asset class."""
    name: str
    description: str
    source: str
    start_year: int
    end_year: int
    num_years: int
    annual_returns: list[float]
    arithmetic_mean: float
    geometric_mean: float
    std_dev: float
    min_return: float
    max_return: float
    skewness: float
    kurtosis: float  # Excess kurtosis (normal = 0)

    def to_dict(self) -> dict:
        return {
            "name": self.name,
            "description": self.description,
            "source": self.source,
            "start_year": self.start_year,
            "end_year": self.end_year,
            "num_years": self.num_years,
            "arithmetic_mean": self.arithmetic_mean,
            "geometric_mean": self.geometric_mean,
            "std_dev": self.std_dev,
            "min_return": self.min_return,
            "max_return": self.max_return,
            "skewness": self.skewness,
            "kurtosis": self.kurtosis,
        }


def compute_stats(name: str, description: str, source: str, returns: pd.Series) -> AssetStats:
    """Compute statistics from a series of annual returns."""
    returns = returns.dropna()
    arr = returns.values.astype(float)

    if len(arr) == 0:
        raise ValueError(f"No valid returns data for {name}")

    # Geometric mean: (prod(1 + r))^(1/n) - 1
    # Handle potential negative total returns
    cumulative = np.prod(1 + arr)
    if cumulative > 0:
        geo_mean = cumulative ** (1 / len(arr)) - 1
    else:
        geo_mean = -1.0  # Total loss

    return AssetStats(
        name=name,
        description=description,
        source=source,
        start_year=int(returns.index.min()),
        end_year=int(returns.index.max()),
        num_years=len(arr),
        annual_returns=arr.tolist(),
        arithmetic_mean=float(np.mean(arr)),
        geometric_mean=float(geo_mean),
        std_dev=float(np.std(arr, ddof=1)),  # Sample std dev
        min_return=float(np.min(arr)),
        max_return=float(np.max(arr)),
        skewness=float(pd.Series(arr).skew()),
        kurtosis=float(pd.Series(arr).kurtosis()),  # Excess kurtosis
    )


# ============================================================================
# Shiller Data (Yale) - S&P 500 back to 1871
# http://www.econ.yale.edu/~shiller/data.htm
# ============================================================================

SHILLER_DATA_URL = "http://www.econ.yale.edu/~shiller/data/ie_data.xls"


def fetch_shiller_data() -> pd.DataFrame:
    """
    Fetch Robert Shiller's historical S&P 500 data.

    Returns DataFrame with columns: Date, Price, Dividend, Earnings, CPI,
    Long_Rate, Real_Price, Real_Dividend, Real_TR_Price, CAPE
    """
    if not HAS_OPENPYXL:
        raise ImportError("openpyxl required for Shiller data")

    print("  Fetching Shiller data from Yale...")

    # Download the Excel file (with caching)
    data = fetch_url_cached(SHILLER_DATA_URL, ".xls")

    # Read the Data sheet, skipping header rows
    df = pd.read_excel(
        io.BytesIO(data),
        sheet_name="Data",
        skiprows=7,  # Skip header explanation rows
        usecols="A:M",
    )

    # Rename columns
    df.columns = [
        'Date', 'Price', 'Dividend', 'Earnings', 'CPI', 'Date_Fraction',
        'Long_Rate', 'Real_Price', 'Real_Dividend', 'Real_TR_Price',
        'Real_Earnings', 'Real_TR_Scaled', 'CAPE'
    ]

    # Convert date (format: YYYY.MM as float, e.g., 1871.01)
    df = df[df['Date'].notna() & (df['Date'] != 'Date')]
    df['Date'] = pd.to_numeric(df['Date'], errors='coerce')
    df = df[df['Date'].notna()]

    df['Year'] = df['Date'].astype(int)
    df['Month'] = ((df['Date'] % 1) * 100).round().astype(int)

    return df


def fetch_shiller_sp500_returns(start_year: int = 1871) -> AssetStats:
    """
    Fetch S&P 500 total returns from Shiller data.

    Shiller's data contains:
    - Price: S&P Composite index level
    - Dividend: Annualized dividend rate (12-month trailing dividend / 12)

    We compute total returns using the formula:
    Total Return = (P1 / P0) * (1 + D/P) - 1
    where D/P is the average dividend yield over the year.
    """
    df = fetch_shiller_data()

    # Filter to start year
    df = df[df['Year'] >= start_year]

    # Get January values for each year to compute year-over-year returns
    # Use first month of year for cleaner annual returns
    jan_data = df[df['Month'] == 1].copy()
    jan_data = jan_data.set_index('Year')

    # Price return (January to January)
    price_returns = jan_data['Price'].pct_change()

    # Dividend yield: Shiller's Dividend column is annualized dividend rate
    # Dividend yield = D / P (already annualized)
    # We use the average dividend yield over the year
    yearly_avg_div = df.groupby('Year')['Dividend'].mean()
    yearly_avg_price = df.groupby('Year')['Price'].mean()
    dividend_yield = yearly_avg_div / yearly_avg_price

    # Total return = price return + dividend yield
    # Align indices
    total_returns = price_returns + dividend_yield.reindex(price_returns.index)
    total_returns = total_returns.dropna()

    # Filter to complete years only
    current_year = datetime.now().year
    total_returns = total_returns[total_returns.index < current_year]

    return compute_stats(
        "S&P 500",
        "US Large Cap Stocks (S&P 500 Total Return)",
        "Robert Shiller, Yale University",
        total_returns,
    )


def fetch_shiller_inflation(start_year: int = 1871) -> AssetStats:
    """Fetch CPI inflation from Shiller data."""
    df = fetch_shiller_data()
    df = df[df['Year'] >= start_year]

    # Get December CPI for each year
    yearly_cpi = df.groupby('Year')['CPI'].last()

    # Compute annual inflation
    inflation = yearly_cpi.pct_change().dropna()

    # Filter to complete years
    current_year = datetime.now().year
    inflation = inflation[inflation.index < current_year]

    return compute_stats(
        "US Inflation",
        "US CPI Inflation (All Items)",
        "Robert Shiller, Yale University (BLS CPI data)",
        inflation,
    )


def fetch_shiller_bond_returns(start_year: int = 1871) -> AssetStats:
    """
    Estimate long-term bond returns from Shiller's long-term interest rate data.
    Uses a simple duration-based approximation for price changes.
    """
    df = fetch_shiller_data()
    df = df[df['Year'] >= start_year]

    # Get yearly interest rates (GS10 equivalent - 10-year Treasury)
    yearly = df.groupby('Year')['Long_Rate'].mean()

    # Approximate bond total return:
    # Return ≈ Yield + Duration * (Change in Yield)
    # Assume duration ≈ 8 years for long-term bonds
    duration = 8.0

    # Yield as decimal
    yields = yearly / 100
    yield_changes = yields.diff()

    # Total return = starting yield - duration * yield change
    # (Price falls when yields rise)
    total_returns = yields.shift(1) - duration * yield_changes
    total_returns = total_returns.dropna()

    # Filter to complete years
    current_year = datetime.now().year
    total_returns = total_returns[total_returns.index < current_year]

    return compute_stats(
        "US Long-Term Bonds",
        "US Long-Term Government Bonds (estimated from yields)",
        "Robert Shiller, Yale University (estimated)",
        total_returns,
    )


# ============================================================================
# Kenneth French Data Library (Dartmouth)
# https://mba.tuck.dartmouth.edu/pages/faculty/ken.french/data_library.html
# ============================================================================

FRENCH_BASE_URL = "https://mba.tuck.dartmouth.edu/pages/faculty/ken.french/ftp/"


def fetch_french_csv(filename: str) -> str:
    """Download and parse a Kenneth French data file."""
    url = f"{FRENCH_BASE_URL}{filename}"
    print(f"  Fetching {filename} from Kenneth French Data Library...")

    # Download with caching
    data = fetch_url_cached(url, ".zip")

    # French data comes as ZIP files containing CSV
    with zipfile.ZipFile(io.BytesIO(data)) as zf:
        # Get the CSV file (usually only one file in the zip)
        csv_name = [n for n in zf.namelist() if n.endswith('.CSV') or n.endswith('.csv')][0]
        with zf.open(csv_name) as f:
            content = f.read().decode('utf-8')

    return content


def parse_french_annual_data(content: str, value_column: int = 1) -> pd.Series:
    """
    Parse Kenneth French data format.

    French files have annual data after a blank line following monthly data.
    Format: YYYY, value1, value2, ...
    """
    lines = content.strip().split('\n')

    # Find annual data section (after monthly, before any other sections)
    in_annual = False
    annual_data = []

    for line in lines:
        line = line.strip()

        # Skip empty lines and headers
        if not line or line.startswith('Average'):
            if in_annual:
                break  # End of annual section
            continue

        # Check if this looks like annual data (4-digit year)
        parts = [p.strip() for p in line.split(',')]
        if len(parts) >= 2:
            try:
                year = int(parts[0])
                if 1900 <= year <= 2100:  # Looks like a year
                    if len(parts[0]) == 4:  # Annual data has 4-digit years
                        in_annual = True
                        value = float(parts[value_column])
                        annual_data.append((year, value / 100))  # Convert percentage to decimal
            except (ValueError, IndexError):
                continue

    if not annual_data:
        raise ValueError("No annual data found in French data file")

    series = pd.Series(
        [v for _, v in annual_data],
        index=[y for y, _ in annual_data]
    )
    return series


def fetch_french_market_returns(start_year: int = 1926) -> AssetStats:
    """
    Fetch US market returns from Fama-French factors.
    Uses Mkt-RF + RF to get total market return.
    """
    content = fetch_french_csv("F-F_Research_Data_Factors_CSV.zip")

    # Parse the data - need both Mkt-RF (col 1) and RF (col 4)
    lines = content.strip().split('\n')

    annual_data = []
    in_annual = False

    for line in lines:
        line = line.strip()
        if not line:
            continue

        parts = [p.strip() for p in line.split(',')]
        if len(parts) >= 5:
            try:
                year = int(parts[0])
                if 1900 <= year <= 2100 and len(parts[0]) == 4:
                    in_annual = True
                    mkt_rf = float(parts[1]) / 100  # Market excess return
                    rf = float(parts[4]) / 100      # Risk-free rate
                    total_return = mkt_rf + rf
                    annual_data.append((year, total_return))
            except (ValueError, IndexError):
                continue

    series = pd.Series(
        [v for _, v in annual_data],
        index=[y for y, _ in annual_data]
    )
    series = series[series.index >= start_year]

    # Remove current incomplete year
    current_year = datetime.now().year
    series = series[series.index < current_year]

    return compute_stats(
        "US Market",
        "US Total Stock Market (Fama-French Market Factor)",
        "Kenneth French Data Library, Dartmouth",
        series,
    )


def fetch_french_smb_returns(start_year: int = 1926) -> AssetStats:
    """
    Fetch Small Minus Big (SMB) factor returns.
    This represents the return premium of small stocks over large stocks.
    """
    content = fetch_french_csv("F-F_Research_Data_Factors_CSV.zip")

    lines = content.strip().split('\n')
    annual_data = []

    for line in lines:
        line = line.strip()
        if not line:
            continue

        parts = [p.strip() for p in line.split(',')]
        if len(parts) >= 3:
            try:
                year = int(parts[0])
                if 1900 <= year <= 2100 and len(parts[0]) == 4:
                    smb = float(parts[2]) / 100  # SMB factor
                    annual_data.append((year, smb))
            except (ValueError, IndexError):
                continue

    series = pd.Series(
        [v for _, v in annual_data],
        index=[y for y, _ in annual_data]
    )
    series = series[series.index >= start_year]

    current_year = datetime.now().year
    series = series[series.index < current_year]

    return compute_stats(
        "Small Cap Premium",
        "Small Minus Big (SMB) Factor - Small Cap Premium over Large Cap",
        "Kenneth French Data Library, Dartmouth",
        series,
    )


def fetch_french_small_cap_returns(start_year: int = 1926) -> AssetStats:
    """
    Fetch small cap returns by adding SMB to market return.
    Small Cap Return ≈ Market Return + SMB
    """
    content = fetch_french_csv("F-F_Research_Data_Factors_CSV.zip")

    lines = content.strip().split('\n')
    annual_data = []

    for line in lines:
        line = line.strip()
        if not line:
            continue

        parts = [p.strip() for p in line.split(',')]
        if len(parts) >= 5:
            try:
                year = int(parts[0])
                if 1900 <= year <= 2100 and len(parts[0]) == 4:
                    mkt_rf = float(parts[1]) / 100
                    smb = float(parts[2]) / 100
                    rf = float(parts[4]) / 100
                    # Small cap ≈ Market + SMB
                    small_cap = mkt_rf + rf + smb
                    annual_data.append((year, small_cap))
            except (ValueError, IndexError):
                continue

    series = pd.Series(
        [v for _, v in annual_data],
        index=[y for y, _ in annual_data]
    )
    series = series[series.index >= start_year]

    current_year = datetime.now().year
    series = series[series.index < current_year]

    return compute_stats(
        "US Small Cap",
        "US Small Cap Stocks (Market + SMB Factor)",
        "Kenneth French Data Library, Dartmouth",
        series,
    )


def fetch_french_risk_free_rate(start_year: int = 1926) -> AssetStats:
    """Fetch risk-free rate (T-bills) from Fama-French data."""
    content = fetch_french_csv("F-F_Research_Data_Factors_CSV.zip")

    lines = content.strip().split('\n')
    annual_data = []

    for line in lines:
        line = line.strip()
        if not line:
            continue

        parts = [p.strip() for p in line.split(',')]
        if len(parts) >= 5:
            try:
                year = int(parts[0])
                if 1900 <= year <= 2100 and len(parts[0]) == 4:
                    rf = float(parts[4]) / 100
                    annual_data.append((year, rf))
            except (ValueError, IndexError):
                continue

    series = pd.Series(
        [v for _, v in annual_data],
        index=[y for y, _ in annual_data]
    )
    series = series[series.index >= start_year]

    current_year = datetime.now().year
    series = series[series.index < current_year]

    return compute_stats(
        "T-Bills",
        "US Treasury Bills (Risk-Free Rate)",
        "Kenneth French Data Library, Dartmouth",
        series,
    )


def fetch_french_international_returns(start_year: int = 1990) -> AssetStats:
    """Fetch developed ex-US market returns."""
    try:
        content = fetch_french_csv("Developed_ex_US_3_Factors_CSV.zip")

        lines = content.strip().split('\n')
        annual_data = []

        for line in lines:
            line = line.strip()
            if not line:
                continue

            parts = [p.strip() for p in line.split(',')]
            if len(parts) >= 5:
                try:
                    year = int(parts[0])
                    if 1900 <= year <= 2100 and len(parts[0]) == 4:
                        mkt_rf = float(parts[1]) / 100
                        rf = float(parts[4]) / 100
                        total_return = mkt_rf + rf
                        annual_data.append((year, total_return))
                except (ValueError, IndexError):
                    continue

        series = pd.Series(
            [v for _, v in annual_data],
            index=[y for y, _ in annual_data]
        )
        series = series[series.index >= start_year]

        current_year = datetime.now().year
        series = series[series.index < current_year]

        return compute_stats(
            "International Developed",
            "Developed Markets ex-US (Fama-French)",
            "Kenneth French Data Library, Dartmouth",
            series,
        )
    except Exception as e:
        raise ValueError(f"Could not fetch international data: {e}")


def fetch_french_emerging_returns(start_year: int = 1990) -> AssetStats:
    """Fetch emerging market returns from French data."""
    try:
        content = fetch_french_csv("Emerging_5_Factors_CSV.zip")

        lines = content.strip().split('\n')
        annual_data = []

        for line in lines:
            line = line.strip()
            if not line:
                continue

            parts = [p.strip() for p in line.split(',')]
            if len(parts) >= 6:
                try:
                    year = int(parts[0])
                    if 1900 <= year <= 2100 and len(parts[0]) == 4:
                        mkt_rf = float(parts[1]) / 100
                        rf = float(parts[5]) / 100
                        total_return = mkt_rf + rf

                        # Skip obviously bad data (returns < -90% are suspicious)
                        if total_return > -0.90:
                            annual_data.append((year, total_return))
                except (ValueError, IndexError):
                    continue

        if not annual_data:
            raise ValueError("No valid data found")

        series = pd.Series(
            [v for _, v in annual_data],
            index=[y for y, _ in annual_data]
        )
        series = series[series.index >= start_year]

        current_year = datetime.now().year
        series = series[series.index < current_year]

        # Sanity check: if geometric mean would be negative, data is bad
        cumulative = np.prod(1 + series.values)
        if cumulative <= 0:
            raise ValueError("Data produces invalid geometric mean")

        return compute_stats(
            "Emerging Markets",
            "Emerging Markets (Fama-French)",
            "Kenneth French Data Library, Dartmouth",
            series,
        )
    except Exception as e:
        raise ValueError(f"Could not fetch emerging markets data: {e}")


# ============================================================================
# Damodaran Data (NYU Stern)
# https://pages.stern.nyu.edu/~adamodar/New_Home_Page/datafile/histretSP.html
# ============================================================================

DAMODARAN_URL = "https://pages.stern.nyu.edu/~adamodar/pc/datasets/histretSP.xls"


def fetch_damodaran_data() -> pd.DataFrame:
    """
    Fetch Aswath Damodaran's historical returns dataset.
    Contains S&P 500, T-Bills, T-Bonds, Baa Corporate Bonds, Real Estate, Gold.
    """
    if not HAS_OPENPYXL:
        raise ImportError("openpyxl required for Damodaran data")

    print("  Downloading Damodaran data from NYU Stern...")

    req = Request(DAMODARAN_URL, headers={'User-Agent': 'Mozilla/5.0'})
    with urlopen(req, timeout=30) as response:
        data = response.read()

    # Read the main data sheet
    df = pd.read_excel(
        io.BytesIO(data),
        sheet_name=0,  # First sheet
        skiprows=0,
    )

    return df


def fetch_damodaran_returns(column_name: str, asset_name: str, description: str, start_year: int = 1928) -> AssetStats:
    """Fetch returns for a specific asset from Damodaran data."""
    df = fetch_damodaran_data()

    # Find the year column and the requested data column
    # Damodaran's format varies, so we need to be flexible
    year_col = None
    data_col = None

    for col in df.columns:
        col_str = str(col).lower()
        if 'year' in col_str:
            year_col = col
        if column_name.lower() in col_str:
            data_col = col

    if year_col is None or data_col is None:
        # Try numeric column indices as fallback
        # Typical layout: Year, S&P500, T-Bill, T-Bond, Baa Corp, Real Estate, Gold, CPI
        year_col = df.columns[0]
        col_map = {
            'sp500': 1, 's&p': 1, 'stock': 1,
            't-bill': 2, 'tbill': 2,
            't-bond': 3, 'tbond': 3,
            'baa': 4, 'corporate': 4,
            'real estate': 5, 'realestate': 5,
            'gold': 6,
            'cpi': 7, 'inflation': 7,
        }
        col_idx = col_map.get(column_name.lower())
        if col_idx and col_idx < len(df.columns):
            data_col = df.columns[col_idx]

    if year_col is None or data_col is None:
        raise ValueError(f"Could not find column {column_name} in Damodaran data")

    # Extract data
    df_clean = df[[year_col, data_col]].copy()
    df_clean.columns = ['Year', 'Return']

    # Clean up - remove non-numeric rows
    df_clean = df_clean[pd.to_numeric(df_clean['Year'], errors='coerce').notna()]
    df_clean['Year'] = df_clean['Year'].astype(int)
    df_clean = df_clean[df_clean['Year'] >= start_year]

    # Convert returns (may be percentages or decimals)
    df_clean['Return'] = pd.to_numeric(df_clean['Return'], errors='coerce')
    if df_clean['Return'].abs().mean() > 1:  # Likely percentages
        df_clean['Return'] = df_clean['Return'] / 100

    # Remove current incomplete year
    current_year = datetime.now().year
    df_clean = df_clean[df_clean['Year'] < current_year]

    series = pd.Series(df_clean['Return'].values, index=df_clean['Year'].values)

    return compute_stats(
        asset_name,
        description,
        "Aswath Damodaran, NYU Stern",
        series,
    )


# ============================================================================
# Yahoo Finance (fallback for recent data)
# ============================================================================

def fetch_yahoo_annual_returns(
    ticker: str,
    start_year: int = 1970,
    end_year: Optional[int] = None,
) -> pd.Series:
    """Fetch price data from Yahoo Finance and compute annual returns."""
    if not HAS_YFINANCE:
        raise ImportError("yfinance is required")

    end_year = end_year or datetime.now().year

    # Check cache first
    cache_key = f"yahoo_{ticker}_{start_year}_{end_year}"
    cache_path = CACHE_DIR / f"{cache_key}.pkl"

    if is_cache_valid(cache_path):
        print(f"    [Using cached {cache_path.name}]")
        with open(cache_path, 'rb') as f:
            return pickle.load(f)

    print(f"    [Downloading {ticker} from Yahoo Finance...]")

    # Fetch daily adjusted close prices
    data = yf.download(
        ticker,
        start=f"{start_year}-01-01",
        end=f"{end_year + 1}-01-01",
        progress=False,
        auto_adjust=True,
    )

    if data.empty:
        raise ValueError(f"No data returned for {ticker}")

    prices = data["Close"]
    if isinstance(prices, pd.DataFrame):
        prices = prices.iloc[:, 0]

    year_end_prices = prices.resample("YE").last()
    annual_returns = year_end_prices.pct_change().dropna()
    annual_returns.index = annual_returns.index.year

    # Save to cache
    CACHE_DIR.mkdir(exist_ok=True)
    with open(cache_path, 'wb') as f:
        pickle.dump(annual_returns, f)
    print(f"    [Cached to {cache_path.name}]")

    return annual_returns


# ============================================================================
# FRED Data
# ============================================================================

def fetch_fred_series(
    series_id: str,
    api_key: Optional[str] = None,
    start_year: int = 1970,
) -> pd.Series:
    """Fetch data from FRED."""
    if not HAS_FRED:
        raise ImportError("fredapi is required")

    if api_key is None:
        api_key = os.environ.get("FRED_API_KEY")
        if api_key is None:
            raise ValueError(
                "FRED API key required. Set FRED_API_KEY environment variable "
                "or pass api_key parameter. Get a free key at: "
                "https://fred.stlouisfed.org/docs/api/api_key.html"
            )

    fred = Fred(api_key=api_key)
    data = fred.get_series(series_id, observation_start=f"{start_year}-01-01")
    return data


# ============================================================================
# FRED Data Fetchers
# ============================================================================

def fetch_fred_tbills(start_year: int = 1934) -> AssetStats:
    """Fetch T-bill returns from FRED (3-Month Treasury Bill rate)."""
    if not HAS_FRED:
        raise ImportError("fredapi is required")

    api_key = os.environ.get("FRED_API_KEY")
    if not api_key:
        raise ValueError("FRED_API_KEY not set")

    # Check cache
    cache_key = f"fred_TB3MS_{start_year}"
    cache_path = CACHE_DIR / f"{cache_key}.pkl"

    if is_cache_valid(cache_path):
        print(f"    [Using cached {cache_path.name}]")
        with open(cache_path, 'rb') as f:
            annual = pickle.load(f)
    else:
        print("    [Downloading TB3MS from FRED...]")
        fred = Fred(api_key=api_key)

        # TB3MS: 3-Month Treasury Bill Secondary Market Rate (monthly, percent)
        data = fred.get_series("TB3MS", observation_start=f"{start_year}-01-01")

        # Convert to annual returns (average rate for the year)
        annual = data.resample("YE").mean() / 100  # Convert percent to decimal
        annual.index = annual.index.year
        annual = annual.dropna()

        # Save to cache
        CACHE_DIR.mkdir(exist_ok=True)
        with open(cache_path, 'wb') as f:
            pickle.dump(annual, f)
        print(f"    [Cached to {cache_path.name}]")

    # Filter to complete years
    current_year = datetime.now().year
    annual = annual[annual.index < current_year]

    return compute_stats(
        "T-Bills",
        "US 3-Month Treasury Bills",
        "FRED (TB3MS)",
        annual,
    )


def fetch_fred_inflation(start_year: int = 1947) -> AssetStats:
    """Fetch CPI inflation from FRED."""
    if not HAS_FRED:
        raise ImportError("fredapi is required")

    api_key = os.environ.get("FRED_API_KEY")
    if not api_key:
        raise ValueError("FRED_API_KEY not set")

    # Check cache
    cache_key = f"fred_CPIAUCSL_{start_year}"
    cache_path = CACHE_DIR / f"{cache_key}.pkl"

    if is_cache_valid(cache_path):
        print(f"    [Using cached {cache_path.name}]")
        with open(cache_path, 'rb') as f:
            inflation = pickle.load(f)
    else:
        print("    [Downloading CPIAUCSL from FRED...]")
        fred = Fred(api_key=api_key)

        # CPIAUCSL: Consumer Price Index for All Urban Consumers
        data = fred.get_series("CPIAUCSL", observation_start=f"{start_year}-01-01")

        # Get December value for each year and compute annual inflation
        year_end = data.resample("YE").last()
        inflation = year_end.pct_change().dropna()
        inflation.index = inflation.index.year

        # Save to cache
        CACHE_DIR.mkdir(exist_ok=True)
        with open(cache_path, 'wb') as f:
            pickle.dump(inflation, f)
        print(f"    [Cached to {cache_path.name}]")

    # Filter to complete years
    current_year = datetime.now().year
    inflation = inflation[inflation.index < current_year]

    return compute_stats(
        "US Inflation",
        "US CPI Inflation (All Urban Consumers)",
        "FRED (CPIAUCSL)",
        inflation,
    )


def fetch_fred_10yr_treasury(start_year: int = 1953) -> AssetStats:
    """
    Fetch 10-Year Treasury yield from FRED and estimate total returns.
    Uses duration-based approximation for price changes.
    """
    if not HAS_FRED:
        raise ImportError("fredapi is required")

    api_key = os.environ.get("FRED_API_KEY")
    if not api_key:
        raise ValueError("FRED_API_KEY not set")

    # Check cache
    cache_key = f"fred_GS10_{start_year}"
    cache_path = CACHE_DIR / f"{cache_key}.pkl"

    if is_cache_valid(cache_path):
        print(f"    [Using cached {cache_path.name}]")
        with open(cache_path, 'rb') as f:
            total_returns = pickle.load(f)
    else:
        print("    [Downloading GS10 from FRED...]")
        fred = Fred(api_key=api_key)

        # GS10: 10-Year Treasury Constant Maturity Rate
        data = fred.get_series("GS10", observation_start=f"{start_year}-01-01")

        # Get annual average yield
        yearly = data.resample("YE").mean()
        yearly.index = yearly.index.year

        # Estimate bond total return using duration approximation
        # Duration for 10-year bond ≈ 8 years
        duration = 8.0
        yields = yearly / 100  # Convert percent to decimal
        yield_changes = yields.diff()

        # Total return ≈ starting yield - duration * yield change
        total_returns = yields.shift(1) - duration * yield_changes
        total_returns = total_returns.dropna()

        # Save to cache
        CACHE_DIR.mkdir(exist_ok=True)
        with open(cache_path, 'wb') as f:
            pickle.dump(total_returns, f)
        print(f"    [Cached to {cache_path.name}]")

    # Filter to complete years
    current_year = datetime.now().year
    total_returns = total_returns[total_returns.index < current_year]

    return compute_stats(
        "US 10-Year Treasury",
        "US 10-Year Treasury Bonds (estimated from yields)",
        "FRED (GS10, estimated)",
        total_returns,
    )


# ============================================================================
# Composite Fetchers (try multiple sources)
# ============================================================================

def fetch_sp500_best(start_year: int = 1871) -> AssetStats:
    """Fetch S&P 500 from best available source."""
    errors = []

    # Try Shiller first (longest history)
    if HAS_OPENPYXL:
        try:
            return fetch_shiller_sp500_returns(start_year)
        except Exception as e:
            errors.append(f"Shiller: {e}")

    # Try French data (1926+)
    try:
        return fetch_french_market_returns(max(start_year, 1926))
    except Exception as e:
        errors.append(f"French: {e}")

    # Fall back to Yahoo Finance
    if HAS_YFINANCE:
        try:
            returns = fetch_yahoo_annual_returns("^SP500TR", max(start_year, 1988))
            return compute_stats(
                "S&P 500",
                "US Large Cap Stocks (S&P 500 Total Return)",
                "Yahoo Finance",
                returns,
            )
        except Exception as e:
            errors.append(f"Yahoo: {e}")

    raise ValueError(f"Could not fetch S&P 500 data: {'; '.join(errors)}")


def fetch_small_cap_best(start_year: int = 1926) -> AssetStats:
    """Fetch small cap returns from best available source."""
    errors = []

    # Try French data first (longest history)
    try:
        return fetch_french_small_cap_returns(start_year)
    except Exception as e:
        errors.append(f"French: {e}")

    # Fall back to Yahoo Finance
    if HAS_YFINANCE:
        try:
            returns = fetch_yahoo_annual_returns("^RUT", 1988)
            return compute_stats(
                "US Small Cap",
                "US Small Cap Stocks (Russell 2000)",
                "Yahoo Finance",
                returns,
            )
        except Exception as e:
            errors.append(f"Yahoo: {e}")

    raise ValueError(f"Could not fetch small cap data: {'; '.join(errors)}")


def fetch_tbills_best(start_year: int = 1926) -> AssetStats:
    """Fetch T-bill returns from best available source."""
    errors = []

    # Try FRED first (longer history, more accurate)
    if HAS_FRED and os.environ.get("FRED_API_KEY"):
        try:
            return fetch_fred_tbills(max(start_year, 1934))
        except Exception as e:
            errors.append(f"FRED: {e}")

    # Try French data
    try:
        return fetch_french_risk_free_rate(start_year)
    except Exception as e:
        errors.append(f"French: {e}")

    raise ValueError(f"Could not fetch T-bill data: {'; '.join(errors)}")


def fetch_bonds_best(start_year: int = 1871) -> AssetStats:
    """Fetch long-term bond returns from best available source."""
    errors = []

    # Try Shiller first (longest history, but estimated)
    if HAS_OPENPYXL:
        try:
            return fetch_shiller_bond_returns(start_year)
        except Exception as e:
            errors.append(f"Shiller: {e}")

    # Fall back to Yahoo Finance (TLT ETF)
    if HAS_YFINANCE:
        try:
            returns = fetch_yahoo_annual_returns("TLT", 2002)
            return compute_stats(
                "US Long-Term Bonds",
                "US Long-Term Treasury Bonds (20+ Year via TLT)",
                "Yahoo Finance",
                returns,
            )
        except Exception as e:
            errors.append(f"Yahoo: {e}")

    raise ValueError(f"Could not fetch bond data: {'; '.join(errors)}")


def fetch_inflation_best(start_year: int = 1871) -> AssetStats:
    """Fetch inflation from best available source."""
    errors = []

    # Try FRED first (most accurate, official BLS data)
    if HAS_FRED and os.environ.get("FRED_API_KEY"):
        try:
            return fetch_fred_inflation(max(start_year, 1947))
        except Exception as e:
            errors.append(f"FRED: {e}")

    # Try Shiller (longest history)
    if HAS_OPENPYXL:
        try:
            return fetch_shiller_inflation(start_year)
        except Exception as e:
            errors.append(f"Shiller: {e}")

    raise ValueError(f"Could not fetch inflation data: {'; '.join(errors)}")


def fetch_international_best(start_year: int = 1990) -> AssetStats:
    """Fetch international developed market returns."""
    errors = []

    # Try French data first
    try:
        return fetch_french_international_returns(start_year)
    except Exception as e:
        errors.append(f"French: {e}")

    # Fall back to Yahoo Finance
    if HAS_YFINANCE:
        try:
            returns = fetch_yahoo_annual_returns("EFA", 2001)
            return compute_stats(
                "International Developed",
                "International Developed Markets (MSCI EAFE via EFA)",
                "Yahoo Finance",
                returns,
            )
        except Exception as e:
            errors.append(f"Yahoo: {e}")

    raise ValueError(f"Could not fetch international data: {'; '.join(errors)}")


def fetch_emerging_best(start_year: int = 1990) -> AssetStats:
    """Fetch emerging market returns."""
    errors = []

    # Try French data first
    try:
        return fetch_french_emerging_returns(start_year)
    except Exception as e:
        errors.append(f"French: {e}")

    # Fall back to Yahoo Finance
    if HAS_YFINANCE:
        try:
            returns = fetch_yahoo_annual_returns("EEM", 2003)
            return compute_stats(
                "Emerging Markets",
                "Emerging Markets (MSCI EM via EEM)",
                "Yahoo Finance",
                returns,
            )
        except Exception as e:
            errors.append(f"Yahoo: {e}")

    raise ValueError(f"Could not fetch emerging markets data: {'; '.join(errors)}")


def fetch_reits_yahoo() -> AssetStats:
    """Fetch REIT returns from Yahoo Finance."""
    if not HAS_YFINANCE:
        raise ImportError("yfinance is required")

    returns = fetch_yahoo_annual_returns("VNQ", 2004)
    return compute_stats(
        "REITs",
        "US Real Estate Investment Trusts (via VNQ)",
        "Yahoo Finance",
        returns,
    )


def fetch_gold_yahoo() -> AssetStats:
    """Fetch gold returns from Yahoo Finance."""
    if not HAS_YFINANCE:
        raise ImportError("yfinance is required")

    returns = fetch_yahoo_annual_returns("GC=F", 1975)
    return compute_stats(
        "Gold",
        "Gold (via GC=F futures)",
        "Yahoo Finance",
        returns,
    )


def fetch_corporate_bonds_yahoo() -> AssetStats:
    """Fetch corporate bond returns from Yahoo Finance."""
    if not HAS_YFINANCE:
        raise ImportError("yfinance is required")

    returns = fetch_yahoo_annual_returns("LQD", 2002)
    return compute_stats(
        "US Corporate Bonds",
        "US Investment Grade Corporate Bonds (via LQD)",
        "Yahoo Finance",
        returns,
    )


def fetch_tips_yahoo() -> AssetStats:
    """Fetch TIPS returns from Yahoo Finance."""
    if not HAS_YFINANCE:
        raise ImportError("yfinance is required")

    returns = fetch_yahoo_annual_returns("TIP", 2003)
    return compute_stats(
        "TIPS",
        "US Treasury Inflation-Protected Securities (via TIP)",
        "Yahoo Finance",
        returns,
    )


def fetch_aggregate_bonds_yahoo() -> AssetStats:
    """Fetch aggregate bond returns from Yahoo Finance."""
    if not HAS_YFINANCE:
        raise ImportError("yfinance is required")

    returns = fetch_yahoo_annual_returns("AGG", 2003)
    return compute_stats(
        "US Aggregate Bond",
        "US Investment Grade Bonds (Bloomberg Aggregate via AGG)",
        "Yahoo Finance",
        returns,
    )


# ============================================================================
# Output Formatters
# ============================================================================

def compute_student_t_scale(std_dev: float, df: float = 5.0) -> float:
    """
    Compute the scale parameter for Student's t distribution.

    For a Student's t with df degrees of freedom:
    - Standard t has variance = df/(df-2) for df > 2
    - To achieve target std_dev, scale = std_dev * sqrt((df-2)/df)
    """
    if df <= 2:
        raise ValueError("df must be > 2 for finite variance")
    return std_dev * np.sqrt((df - 2) / df)


def format_rust_const(stats: AssetStats, const_prefix: str) -> str:
    """Format statistics as Rust const definitions."""
    rust_name = const_prefix.upper().replace(" ", "_").replace("-", "_")

    lines = [
        f"    // {stats.description}",
        f"    // Source: {stats.source}",
        f"    // Data: {stats.start_year}-{stats.end_year} ({stats.num_years} years)",
        f"    // Arithmetic mean: {stats.arithmetic_mean:.4f}, Geometric mean: {stats.geometric_mean:.4f}",
        f"    // Std dev: {stats.std_dev:.4f}, Skewness: {stats.skewness:.2f}, Kurtosis: {stats.kurtosis:.2f}",
        f"    pub const {rust_name}_HISTORICAL_FIXED: ReturnProfile = ReturnProfile::Fixed({stats.geometric_mean:.6});",
        f"    pub const {rust_name}_HISTORICAL_NORMAL: ReturnProfile = ReturnProfile::Normal {{",
        f"        mean: {stats.arithmetic_mean:.6},",
        f"        std_dev: {stats.std_dev:.6},",
        f"    }};",
    ]

    # Add LogNormal if appropriate
    if stats.arithmetic_mean > 0 and stats.std_dev < stats.arithmetic_mean * 3:
        lines.extend([
            f"    pub const {rust_name}_HISTORICAL_LOGNORMAL: ReturnProfile = ReturnProfile::LogNormal {{",
            f"        mean: {stats.arithmetic_mean:.6},",
            f"        std_dev: {stats.std_dev:.6},",
            f"    }};",
        ])

    # Add Student's t for equity-like assets (std_dev > 5%)
    # Student's t with df=5 captures fat tails better than Normal
    if stats.std_dev > 0.05:
        df = 5.0
        scale = compute_student_t_scale(stats.std_dev, df)
        lines.extend([
            f"    pub const {rust_name}_HISTORICAL_STUDENT_T: ReturnProfile = ReturnProfile::StudentT {{",
            f"        mean: {stats.arithmetic_mean:.6},",
            f"        scale: {scale:.6},",
            f"        df: {df},",
            f"    }};",
        ])

    return "\n".join(lines)


def format_rust_historical_returns(stats: AssetStats, const_prefix: str) -> str:
    """Format historical returns as Rust array for bootstrap sampling."""
    rust_name = const_prefix.upper().replace(" ", "_").replace("-", "_")

    returns_str = ",\n        ".join(
        ", ".join(f"{r:.4f}" for r in stats.annual_returns[i:i+8])
        for i in range(0, len(stats.annual_returns), 8)
    )

    lines = [
        f"    /// {stats.description}",
        f"    /// Source: {stats.source}",
        f"    /// Annual returns {stats.start_year}-{stats.end_year} ({stats.num_years} years)",
        f"    pub const {rust_name}_ANNUAL_RETURNS: &[f64] = &[",
        f"        {returns_str}",
        f"    ];",
    ]

    return "\n".join(lines)


def format_inflation_rust_const(stats: AssetStats) -> str:
    """Format inflation statistics as Rust const definitions."""
    lines = [
        f"    // {stats.description}",
        f"    // Source: {stats.source}",
        f"    // Data: {stats.start_year}-{stats.end_year} ({stats.num_years} years)",
        f"    // Arithmetic mean: {stats.arithmetic_mean:.4f}, Geometric mean: {stats.geometric_mean:.4f}",
        f"    // Std dev: {stats.std_dev:.4f}",
        f"    pub const US_HISTORICAL_FIXED: InflationProfile = InflationProfile::Fixed({stats.geometric_mean:.6});",
        f"    pub const US_HISTORICAL_NORMAL: InflationProfile = InflationProfile::Normal {{",
        f"        mean: {stats.arithmetic_mean:.6},",
        f"        std_dev: {stats.std_dev:.6},",
        f"    }};",
    ]
    return "\n".join(lines)


def format_inflation_rust_array(stats: AssetStats) -> str:
    """Format historical inflation rates as Rust array for bootstrap sampling."""
    # Format compactly with 8 values per line (same as returns arrays)
    returns_str = ",\n        ".join(
        ", ".join(f"{r:.4f}" for r in stats.annual_returns[i:i+8])
        for i in range(0, len(stats.annual_returns), 8)
    )

    lines = [
        f"    /// {stats.description}",
        f"    /// Source: {stats.source}",
        f"    /// Annual rates {stats.start_year}-{stats.end_year} ({stats.num_years} years)",
        f"    /// Arithmetic mean: {stats.arithmetic_mean:.2%}, Geometric mean: {stats.geometric_mean:.2%}, Std dev: {stats.std_dev:.2%}",
        f"    pub const US_CPI_ANNUAL_RATES: &[f64] = &[",
        f"        {returns_str}",
        f"    ];",
    ]

    return "\n".join(lines)


# ============================================================================
# Main
# ============================================================================

def main():
    parser = argparse.ArgumentParser(
        description="Fetch historical return data and generate Rust constants",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Data Sources:
  - Robert Shiller (Yale): S&P 500, CPI, Bond yields (1871+)
  - Kenneth French (Dartmouth): Market, SMB, International (1926+)
  - Yahoo Finance: ETFs for recent data (2000+)
  - FRED: T-bills, inflation (optional, requires API key)

Examples:
  %(prog)s                          # Basic run with academic sources
  %(prog)s --output json            # JSON output
  %(prog)s --include-returns        # Include bootstrap arrays
  %(prog)s --start-year 1926        # Start from 1926
        """
    )
    parser.add_argument(
        "--fred-api-key",
        help="FRED API key (or set FRED_API_KEY env var)",
    )
    parser.add_argument(
        "--output",
        choices=["rust", "json", "csv"],
        default="rust",
        help="Output format (default: rust)",
    )
    parser.add_argument(
        "--include-returns",
        action="store_true",
        help="Include full annual returns arrays for bootstrap",
    )
    parser.add_argument(
        "--start-year",
        type=int,
        default=1926,
        help="Start year for data (default: 1926, min for long history: 1871)",
    )
    parser.add_argument(
        "--clear-cache",
        action="store_true",
        help="Clear cached data and re-download everything",
    )
    parser.add_argument(
        "--cache-days",
        type=int,
        default=7,
        help="Cache validity in days (default: 7)",
    )
    args = parser.parse_args()

    # Handle cache clearing
    global CACHE_MAX_AGE_DAYS
    CACHE_MAX_AGE_DAYS = args.cache_days

    if args.clear_cache:
        import shutil
        if CACHE_DIR.exists():
            shutil.rmtree(CACHE_DIR)
            print(f"Cleared cache directory: {CACHE_DIR}")
        print()

    # Load environment variables from .env file
    load_dotenv()

    # Use FRED API key from args, env var, or .env file
    fred_api_key = args.fred_api_key or os.environ.get("FRED_API_KEY")

    print("=" * 70)
    print("Historical Returns Data Fetcher")
    print("Sources: Shiller (Yale), French (Dartmouth), Yahoo Finance")
    print("=" * 70)
    print()

    all_stats: list[tuple[str, AssetStats]] = []

    # Core asset classes with long history
    fetchers = [
        ("SP_500", "S&P 500", lambda: fetch_sp500_best(args.start_year)),
        ("US_SMALL_CAP", "US Small Cap", lambda: fetch_small_cap_best(args.start_year)),
        ("US_TBILLS", "T-Bills", lambda: fetch_tbills_best(args.start_year)),
        ("US_LONG_BOND", "Long-Term Bonds", lambda: fetch_bonds_best(args.start_year)),
        ("INTL_DEVELOPED", "Intl Developed", lambda: fetch_international_best(1990)),
        ("EMERGING_MARKETS", "Emerging Markets", lambda: fetch_emerging_best(1990)),
    ]

    # Additional asset classes (Yahoo Finance)
    if HAS_YFINANCE:
        fetchers.extend([
            ("REITS", "REITs", fetch_reits_yahoo),
            ("GOLD", "Gold", fetch_gold_yahoo),
            ("US_AGG_BOND", "Aggregate Bonds", fetch_aggregate_bonds_yahoo),
            ("US_CORPORATE_BOND", "Corporate Bonds", fetch_corporate_bonds_yahoo),
            ("TIPS", "TIPS", fetch_tips_yahoo),
        ])

    for prefix, name, fetcher in fetchers:
        try:
            print(f"Fetching {name}...")
            stats = fetcher()
            all_stats.append((prefix, stats))
            print(f"  ✓ {stats.start_year}-{stats.end_year} ({stats.num_years} years): "
                  f"mean={stats.arithmetic_mean:.2%}, geo={stats.geometric_mean:.2%}, "
                  f"std={stats.std_dev:.2%}")
            print(f"    Source: {stats.source}")
        except Exception as e:
            print(f"  ✗ ERROR: {e}")
        print()

    # Fetch inflation
    inflation_stats = None
    try:
        print("Fetching Inflation...")
        inflation_stats = fetch_inflation_best(args.start_year)
        print(f"  ✓ {inflation_stats.start_year}-{inflation_stats.end_year} ({inflation_stats.num_years} years): "
              f"mean={inflation_stats.arithmetic_mean:.2%}, std={inflation_stats.std_dev:.2%}")
        print(f"    Source: {inflation_stats.source}")
    except Exception as e:
        print(f"  ✗ ERROR: {e}")
    print()

    print("=" * 70)
    print("OUTPUT")
    print("=" * 70)
    print()

    if args.output == "json":
        output = {
            "generated_at": datetime.now().isoformat(),
            "sources": [
                "Robert Shiller, Yale University",
                "Kenneth French Data Library, Dartmouth",
                "Yahoo Finance",
            ],
            "return_profiles": {prefix: stats.to_dict() for prefix, stats in all_stats},
        }
        if inflation_stats:
            output["inflation"] = inflation_stats.to_dict()
        print(json.dumps(output, indent=2))

    elif args.output == "csv":
        print("name,source,start_year,end_year,arithmetic_mean,geometric_mean,std_dev,skewness,kurtosis")
        for prefix, stats in all_stats:
            print(f"{prefix},{stats.source},{stats.start_year},{stats.end_year},"
                  f"{stats.arithmetic_mean:.6f},{stats.geometric_mean:.6f},"
                  f"{stats.std_dev:.6f},{stats.skewness:.4f},{stats.kurtosis:.4f}")
        if inflation_stats:
            print(f"INFLATION,{inflation_stats.source},{inflation_stats.start_year},{inflation_stats.end_year},"
                  f"{inflation_stats.arithmetic_mean:.6f},{inflation_stats.geometric_mean:.6f},"
                  f"{inflation_stats.std_dev:.6f},{inflation_stats.skewness:.4f},{inflation_stats.kurtosis:.4f}")

    else:  # rust
        print("// Auto-generated by scripts/fetch_historical_returns.py")
        print(f"// Generated: {datetime.now().isoformat()}")
        print("// ")
        print("// Data Sources:")
        print("//   - Robert Shiller, Yale University (S&P 500 since 1871)")
        print("//   - Kenneth French Data Library, Dartmouth (Fama-French factors since 1926)")
        print("//   - Yahoo Finance (ETF data for recent history)")
        print()
        print("impl ReturnProfile {")
        for prefix, stats in all_stats:
            print(format_rust_const(stats, prefix))
            print()
        print("}")

        if args.include_returns:
            print()
            print("/// Historical annual returns for bootstrap sampling")
            print("pub mod historical_returns {")
            for prefix, stats in all_stats:
                print(format_rust_historical_returns(stats, prefix))
                print()
            print("}")

        if inflation_stats:
            print()
            print("impl InflationProfile {")
            print(format_inflation_rust_const(inflation_stats))
            print("}")

            if args.include_returns:
                print()
                print("/// Historical annual inflation rates for bootstrap sampling")
                print("pub mod historical_inflation {")
                print(format_inflation_rust_array(inflation_stats))
                print("}")

    return 0


if __name__ == "__main__":
    exit(main())
