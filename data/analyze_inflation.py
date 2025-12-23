#!/usr/bin/env python3
import csv
import math
import sys
from pathlib import Path


def read_csv_data(filepath):
    """Read data from CSV file."""
    dates = []
    values = []
    with open(filepath, "r", encoding="utf-8-sig") as f:
        reader = csv.DictReader(f)
        for row in reader:
            dates.append(row["Date"])
            values.append(float(row["Value"]))
    return dates, values


def mean(data):
    """Calculate mean of a list."""
    return sum(data) / len(data)


def std_dev(data, sample=True):
    """Calculate standard deviation of a list."""
    n = len(data)
    m = mean(data)
    variance = sum((x - m) ** 2 for x in data) / (n - 1 if sample else n)
    return math.sqrt(variance)


def analyze_data(filepath):
    """Analyze data from CSV file and print statistics."""
    # Read the data
    dates, raw_values = read_csv_data(filepath)

    # Convert values from percentage to decimal
    decimal_values = [v / 100 for v in raw_values]

    # Determine data type from filename
    filename = Path(filepath).name.lower()
    if "inflation" in filename:
        data_type = "INFLATION"
        value_name = "inflation"
    elif (
        "return" in filename
        or "dow" in filename
        or "sp" in filename
        or "s&p" in filename
    ):
        data_type = "RETURNS"
        value_name = "return"
    else:
        data_type = "DATA"
        value_name = "value"

    print("=" * 80)
    print(f"{data_type} ANALYSIS")
    print(f"File: {Path(filepath).name}")
    print("=" * 80)
    print(f"Data period: {dates[0]} to {dates[-1]}")
    print(f"Number of years: {len(decimal_values)}")

    print(f"Average annual {value_name}: {mean(decimal_values) * 100:.4f}%")
    print(
        f"Median annual {value_name}: {sorted(decimal_values)[len(decimal_values) // 2] * 100:.4f}%"
    )
    print(f"Minimum {value_name}: {min(decimal_values) * 100:.4f}%")
    print(f"Maximum {value_name}: {max(decimal_values) * 100:.4f}%")

    # Normal Distribution Parameters
    mean_normal = mean(decimal_values)
    std_normal = std_dev(decimal_values)

    print(f"\nMean (μ): {mean_normal:.6f} ({mean_normal * 100:.4f}%)")
    print(f"Std Dev (σ): {std_normal:.6f} ({std_normal * 100:.4f}%)")
    print(
        f"\nNormal: μ={mean_normal:.6f} ({mean_normal * 100:.4f}%), σ={std_normal:.6f} ({std_normal * 100:.4f}%)"
    )

    # Lognormal Distribution Parameters
    # For lognormal, we need to transform the data
    # We add 1 to rates to avoid log of negative numbers
    # (since 1 + rate represents the growth multiplier)
    growth_multipliers = [1 + r for r in decimal_values]

    # Filter out any non-positive values
    positive_multipliers = [m for m in growth_multipliers if m > 0]

    if len(positive_multipliers) < len(growth_multipliers):
        negative_count = len(growth_multipliers) - len(positive_multipliers)
        print(
            f"Warning: {negative_count} years had {value_name} < -100% (excluded from lognormal)"
        )

    # Calculate lognormal parameters
    if positive_multipliers:
        log_values = [math.log(m) for m in positive_multipliers]
        mu_lognormal = mean(log_values)
        sigma_lognormal = std_dev(log_values)

        print(f"Lognormal: μ={mu_lognormal:.6f}, σ={sigma_lognormal:.6f}")
    else:
        print("Lognormal: Cannot calculate (no positive growth multipliers)")

    # Modern era analysis (post-1950)
    print("\n" + "=" * 80)
    print("MODERN ERA ANALYSIS (1950-present)")
    print("=" * 80)

    modern_indices = [i for i, d in enumerate(dates) if d >= "12/31/1950"]

    if not modern_indices:
        print("No data available for modern era (1950-present)")
        return

    modern_values = [decimal_values[i] for i in modern_indices]

    print(f"Number of years: {len(modern_values)}")
    print(f"Average annual {value_name}: {mean(modern_values) * 100:.4f}%")

    # Modern Normal Distribution
    mean_modern_normal = mean(modern_values)
    std_modern_normal = std_dev(modern_values)
    print(
        f"\nNormal: μ={mean_modern_normal:.6f} ({mean_modern_normal * 100:.4f}%), σ={std_modern_normal:.6f} ({std_modern_normal * 100:.4f}%)"
    )

    # Modern Lognormal Distribution
    modern_multipliers = [1 + r for r in modern_values]
    positive_modern_multipliers = [m for m in modern_multipliers if m > 0]

    if positive_modern_multipliers:
        log_modern = [math.log(m) for m in positive_modern_multipliers]
        mu_modern_lognormal = mean(log_modern)
        sigma_modern_lognormal = std_dev(log_modern)
        print(f"Lognormal: μ={mu_modern_lognormal:.6f}, σ={sigma_modern_lognormal:.6f}")
    else:
        print("Lognormal: Cannot calculate (no positive growth multipliers)")


def main():
    if len(sys.argv) < 2:
        print("Usage: python analyze_inflation.py <csv_file>")
        print("\nExamples:")
        print("  python analyze_inflation.py inflation/annual_inflation.csv")
        print("  python analyze_inflation.py returns/dow_annual_returns.csv")
        print("  python analyze_inflation.py returns/sp_500_annual_returns.csv")
        sys.exit(1)

    csv_file = sys.argv[1]

    # Convert to Path and resolve
    csv_path = Path(csv_file)

    if not csv_path.exists():
        # If not found, try from script directory
        script_dir = Path(__file__).parent
        csv_path = script_dir / csv_file

    if not csv_path.exists():
        print(f"Error: File not found: {csv_file}")
        sys.exit(1)

    analyze_data(csv_path)


if __name__ == "__main__":
    main()
