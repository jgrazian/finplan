"use client";

import * as React from "react";
import {
    Area,
    AreaChart,
    Bar,
    BarChart,
    CartesianGrid,
    XAxis,
    YAxis,
    ResponsiveContainer,
    Tooltip,
    Legend,
} from "recharts";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
    Table,
    TableBody,
    TableCell,
    TableHead,
    TableHeader,
    TableRow,
} from "@/components/ui/table";
import { Badge } from "@/components/ui/badge";
import { ChartContainer, ChartTooltipContent, ChartLegend, ChartLegendContent, ChartTooltip } from "@/components/ui/chart";
import { AggregatedResult, TimePointStats, SimulationParameters, AccountType } from "@/lib/types";

interface ResultsDashboardProps {
    result: AggregatedResult;
    simulationName?: string;
    simulationParameters?: SimulationParameters;
}

const chartConfig = {
    p10: {
        label: "10th Percentile",
        color: "hsl(var(--chart-1))",
    },
    p50: {
        label: "Median",
        color: "hsl(var(--chart-2))",
    },
    p90: {
        label: "90th Percentile",
        color: "hsl(var(--chart-3))",
    },
};

const accountTypeConfig = {
    Taxable: {
        label: "Taxable",
        color: "var(--chart-1)",
    },
    TaxDeferred: {
        label: "Tax-Deferred",
        color: "var(--chart-2)",
    },
    TaxFree: {
        label: "Tax-Free",
        color: "var(--chart-3)",
    },
    Illiquid: {
        label: "Illiquid",
        color: "var(--chart-4)",
    },
    Debt: {
        label: "Debt",
        color: "var(--chart-5)",
    },
};

const growthComponentsConfig = {
    principal: {
        label: "Initial Principal",
        color: "var(--chart-1)",
    },
    contributions: {
        label: "Contributions",
        color: "var(--chart-2)",
    },
    returns: {
        label: "Investment Returns",
        color: "var(--chart-3)",
    },
    withdrawals: {
        label: "Withdrawals",
        color: "var(--chart-4)",
    },
};

export function ResultsDashboard({ result, simulationName, simulationParameters }: ResultsDashboardProps) {
    const formatCurrency = (value: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            notation: "compact",
            maximumFractionDigits: 1,
        }).format(value);

    const formatFullCurrency = (value: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(value);

    // Prepare chart data
    const chartData = result.total_portfolio.map((point) => ({
        date: new Date(point.date).getFullYear().toString(),
        fullDate: point.date,
        p10: point.p10,
        p50: point.p50,
        p90: point.p90,
    }));

    // Get summary statistics
    const initialValue = result.total_portfolio[0];
    const finalValue = result.total_portfolio[result.total_portfolio.length - 1];
    const peakValue = result.total_portfolio.reduce(
        (max, point) => (point.p50 > max.p50 ? point : max),
        result.total_portfolio[0]
    );
    const minValue = result.total_portfolio.reduce(
        (min, point) => (point.p50 < min.p50 ? point : min),
        result.total_portfolio[0]
    );

    // Calculate success rate (ending value > 0)
    const successRate = finalValue.p10 > 0 ? 100 : finalValue.p50 > 0 ? 50 : 0;

    // Get account data
    const accountIds = Object.keys(result.accounts);

    // Build account type to account IDs mapping
    const accountTypeMap = React.useMemo(() => {
        const map: Record<string, AccountType> = {};
        if (simulationParameters?.accounts) {
            simulationParameters.accounts.forEach((acc) => {
                map[acc.account_id.toString()] = acc.account_type;
            });
        }
        return map;
    }, [simulationParameters]);

    // Check if an account has any liabilities
    const hasLiabilities = React.useMemo(() => {
        const liabilityAccounts: Record<string, boolean> = {};
        if (simulationParameters?.accounts) {
            simulationParameters.accounts.forEach((acc) => {
                const hasLiability = acc.assets.some((asset) => asset.asset_class === "Liability");
                if (hasLiability) {
                    liabilityAccounts[acc.account_id.toString()] = true;
                }
            });
        }
        return liabilityAccounts;
    }, [simulationParameters]);

    // Prepare portfolio breakdown data by account type (yearly, using median values)
    const portfolioBreakdownData = React.useMemo(() => {
        // Group data by year and take the last data point for each year
        const yearlyDataMap = new Map<string, typeof result.total_portfolio[0]>();

        result.total_portfolio.forEach((point) => {
            const year = new Date(point.date).getFullYear().toString();
            yearlyDataMap.set(year, point); // Keep overwriting to get the last point of each year
        });

        // Convert to array and sort by year
        const yearlyData = Array.from(yearlyDataMap.entries())
            .sort(([a], [b]) => a.localeCompare(b))
            .map(([year, point]) => {
                const breakdown: { year: string; Taxable: number; TaxDeferred: number; TaxFree: number; Illiquid: number; Debt: number } = {
                    year,
                    Taxable: 0,
                    TaxDeferred: 0,
                    TaxFree: 0,
                    Illiquid: 0,
                    Debt: 0,
                };

                // Aggregate account values by type
                Object.entries(result.accounts).forEach(([accountId, accountData]) => {
                    const matchingPoint = accountData.find((p) => new Date(p.date).getFullYear().toString() === year);
                    if (matchingPoint) {
                        const accountType = accountTypeMap[accountId] || "Taxable";
                        const isLiability = hasLiabilities[accountId];

                        if (isLiability) {
                            // Show liabilities as negative (debt)
                            breakdown.Debt -= Math.abs(matchingPoint.p50);
                        } else {
                            breakdown[accountType] += matchingPoint.p50;
                        }
                    }
                });

                return breakdown;
            });

        return yearlyData;
    }, [result, accountTypeMap, hasLiabilities]);

    // Determine which account types have data
    const activeAccountTypes = React.useMemo(() => {
        const types = new Set<string>();
        portfolioBreakdownData.forEach((point) => {
            if (point.Taxable > 0) types.add("Taxable");
            if (point.TaxDeferred > 0) types.add("TaxDeferred");
            if (point.TaxFree > 0) types.add("TaxFree");
            if (point.Illiquid > 0) types.add("Illiquid");
            if (point.Debt < 0) types.add("Debt");
        });
        return Array.from(types);
    }, [portfolioBreakdownData]);

    // Estimate yearly contributions from cash flows
    const estimatedYearlyContributions = React.useMemo(() => {
        if (!simulationParameters) return 0;

        return simulationParameters.cash_flows
            .filter((cf) => cf.source === "External" && cf.state === "Active")
            .reduce((sum, cf) => {
                const multiplier =
                    cf.repeats === "Monthly" ? 12 :
                        cf.repeats === "Yearly" ? 1 :
                            cf.repeats === "Weekly" ? 52 :
                                cf.repeats === "BiWeekly" ? 26 :
                                    cf.repeats === "Quarterly" ? 4 : 0;
                return sum + cf.amount * multiplier;
            }, 0);
    }, [simulationParameters]);

    // Estimate yearly withdrawals from spending targets
    const estimatedYearlyWithdrawals = React.useMemo(() => {
        if (!simulationParameters) return 0;

        return simulationParameters.spending_targets
            .filter((st) => st.state === "Active" || st.state === "Pending")
            .reduce((sum, st) => {
                const multiplier = st.repeats === "Monthly" ? 12 : st.repeats === "Yearly" ? 1 : 0;
                return sum + st.amount * multiplier;
            }, 0);
    }, [simulationParameters]);

    // Prepare growth components data using real transaction logs from backend
    const growthComponentsData = React.useMemo(() => {
        // Use real growth_components from the API if available
        if (result.growth_components && result.growth_components.length > 0) {
            // Calculate cumulative totals for stacked display
            let cumulativeReturns = 0;
            let cumulativeLosses = 0;
            let cumulativeContributions = 0;
            let cumulativeWithdrawals = 0;
            let cumulativeExpenses = 0;

            // Get initial principal from first year's total
            const initialPrincipal = result.total_portfolio[0]?.p50 || 0;

            return result.growth_components.map((gc) => {
                cumulativeReturns += gc.investment_returns;
                cumulativeLosses += gc.losses; // Already negative
                cumulativeContributions += gc.contributions;
                cumulativeWithdrawals += gc.withdrawals; // Already negative
                cumulativeExpenses += gc.cash_flow_expenses; // Already negative

                return {
                    year: gc.year.toString(),
                    principal: initialPrincipal,
                    contributions: cumulativeContributions,
                    returns: cumulativeReturns,
                    // Combine all negative flows
                    withdrawals: cumulativeLosses + cumulativeWithdrawals + cumulativeExpenses,
                };
            });
        }

        // Fallback to estimation if growth_components not available
        if (portfolioBreakdownData.length === 0) return [];

        const initialPrincipal = portfolioBreakdownData[0]
            ? portfolioBreakdownData[0].Taxable + portfolioBreakdownData[0].TaxDeferred +
            portfolioBreakdownData[0].TaxFree + portfolioBreakdownData[0].Illiquid + portfolioBreakdownData[0].Debt
            : 0;

        return portfolioBreakdownData.map((point, index) => {
            const currentTotal = point.Taxable + point.TaxDeferred + point.TaxFree + point.Illiquid + point.Debt;

            if (index === 0) {
                return {
                    year: point.year,
                    principal: Math.max(0, currentTotal),
                    contributions: 0,
                    returns: 0,
                    withdrawals: Math.min(0, currentTotal),
                };
            }

            const prevPoint = portfolioBreakdownData[index - 1];
            const prevTotal = prevPoint.Taxable + prevPoint.TaxDeferred + prevPoint.TaxFree + prevPoint.Illiquid + prevPoint.Debt;
            const yearChange = currentTotal - prevTotal;
            const estContributions = estimatedYearlyContributions;
            const estWithdrawals = -estimatedYearlyWithdrawals;
            const estReturns = yearChange - estContributions - estWithdrawals;

            return {
                year: point.year,
                principal: initialPrincipal,
                contributions: Math.max(0, estContributions * index),
                returns: estReturns > 0 ? estReturns : 0,
                withdrawals: estReturns < 0 ? estReturns : estWithdrawals,
            };
        });
    }, [result.growth_components, portfolioBreakdownData, estimatedYearlyContributions, estimatedYearlyWithdrawals, result.total_portfolio]);

    // Determine which growth components have data
    const activeGrowthComponents = React.useMemo(() => {
        const components = new Set<string>();
        growthComponentsData.forEach((point) => {
            if (point.principal > 0) components.add("principal");
            if (point.contributions > 0) components.add("contributions");
            if (point.returns > 0) components.add("returns");
            if (point.withdrawals < 0) components.add("withdrawals");
        });
        return Array.from(components);
    }, [growthComponentsData]);

    return (
        <div className="space-y-6">
            {/* Summary Cards */}
            <div className="grid gap-4 md:grid-cols-4">
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            Starting Value
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{formatCurrency(initialValue?.p50 || 0)}</p>
                        <p className="text-xs text-muted-foreground">Median projection</p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            Final Value (Median)
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{formatCurrency(finalValue?.p50 || 0)}</p>
                        <p className="text-xs text-muted-foreground">
                            Range: {formatCurrency(finalValue?.p10 || 0)} - {formatCurrency(finalValue?.p90 || 0)}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Peak Value</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{formatCurrency(peakValue?.p50 || 0)}</p>
                        <p className="text-xs text-muted-foreground">
                            In {new Date(peakValue?.date || "").getFullYear()}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            Success Indicator
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <Badge
                            variant={successRate >= 90 ? "default" : successRate >= 50 ? "secondary" : "destructive"}
                            className="text-lg px-3 py-1"
                        >
                            {finalValue?.p10 > 0 ? "Strong" : finalValue?.p50 > 0 ? "Moderate" : "At Risk"}
                        </Badge>
                        <p className="text-xs text-muted-foreground mt-1">
                            {finalValue?.p10 > 0
                                ? "90%+ scenarios end positive"
                                : finalValue?.p50 > 0
                                    ? "50%+ scenarios end positive"
                                    : "High depletion risk"}
                        </p>
                    </CardContent>
                </Card>
            </div>

            {/* Main Chart */}
            <Card>
                <CardHeader>
                    <CardTitle>Portfolio Projection</CardTitle>
                    <CardDescription>
                        Monte Carlo simulation showing 10th, 50th (median), and 90th percentile outcomes
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <ChartContainer config={chartConfig} className="h-[400px] w-full">
                        <ResponsiveContainer width="100%" height="100%">
                            <AreaChart data={chartData} margin={{ top: 10, right: 30, left: 0, bottom: 0 }}>
                                <defs>
                                    <linearGradient id="colorP90" x1="0" y1="0" x2="0" y2="1">
                                        <stop offset="5%" stopColor="hsl(var(--chart-3))" stopOpacity={0.3} />
                                        <stop offset="95%" stopColor="hsl(var(--chart-3))" stopOpacity={0} />
                                    </linearGradient>
                                    <linearGradient id="colorP50" x1="0" y1="0" x2="0" y2="1">
                                        <stop offset="5%" stopColor="hsl(var(--chart-2))" stopOpacity={0.5} />
                                        <stop offset="95%" stopColor="hsl(var(--chart-2))" stopOpacity={0} />
                                    </linearGradient>
                                    <linearGradient id="colorP10" x1="0" y1="0" x2="0" y2="1">
                                        <stop offset="5%" stopColor="hsl(var(--chart-1))" stopOpacity={0.3} />
                                        <stop offset="95%" stopColor="hsl(var(--chart-1))" stopOpacity={0} />
                                    </linearGradient>
                                </defs>
                                <CartesianGrid strokeDasharray="3 3" className="stroke-muted" />
                                <XAxis
                                    dataKey="date"
                                    tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
                                    tickLine={{ stroke: "hsl(var(--muted-foreground))" }}
                                />
                                <YAxis
                                    tickFormatter={(value) => formatCurrency(value)}
                                    tick={{ fill: "hsl(var(--muted-foreground))", fontSize: 12 }}
                                    tickLine={{ stroke: "hsl(var(--muted-foreground))" }}
                                    width={80}
                                />
                                <Tooltip
                                    content={({ active, payload, label }) => {
                                        if (active && payload && payload.length) {
                                            return (
                                                <div className="rounded-lg border bg-background p-3 shadow-sm">
                                                    <p className="font-medium mb-2">{label}</p>
                                                    {payload.map((entry, index) => (
                                                        <p key={index} className="text-sm" style={{ color: entry.color }}>
                                                            {entry.name}: {formatFullCurrency(entry.value as number)}
                                                        </p>
                                                    ))}
                                                </div>
                                            );
                                        }
                                        return null;
                                    }}
                                />
                                <Legend />
                                <Area
                                    type="monotone"
                                    dataKey="p90"
                                    name="90th Percentile"
                                    stroke="hsl(var(--chart-3))"
                                    fillOpacity={1}
                                    fill="url(#colorP90)"
                                />
                                <Area
                                    type="monotone"
                                    dataKey="p50"
                                    name="Median"
                                    stroke="hsl(var(--chart-2))"
                                    fillOpacity={1}
                                    fill="url(#colorP50)"
                                    strokeWidth={2}
                                />
                                <Area
                                    type="monotone"
                                    dataKey="p10"
                                    name="10th Percentile"
                                    stroke="hsl(var(--chart-1))"
                                    fillOpacity={1}
                                    fill="url(#colorP10)"
                                />
                            </AreaChart>
                        </ResponsiveContainer>
                    </ChartContainer>
                </CardContent>
            </Card>

            {/* Portfolio Breakdown by Account Type */}
            {simulationParameters && portfolioBreakdownData.length > 0 && (
                <Card>
                    <CardHeader>
                        <CardTitle>Portfolio Breakdown by Account Type</CardTitle>
                        <CardDescription>
                            Yearly contribution breakdown showing assets by tax treatment. Debts shown below the axis.
                        </CardDescription>
                    </CardHeader>
                    <CardContent>
                        <ChartContainer config={accountTypeConfig} className="h-[400px] w-full">
                            <BarChart
                                accessibilityLayer
                                data={portfolioBreakdownData}
                                margin={{ top: 20, right: 30, left: 0, bottom: 5 }}
                                stackOffset="sign"
                            >
                                <CartesianGrid vertical={false} />
                                <XAxis
                                    dataKey="year"
                                    tickLine={false}
                                    tickMargin={10}
                                    axisLine={false}
                                />
                                <YAxis
                                    tickFormatter={(value) => formatCurrency(value)}
                                    tickLine={false}
                                    axisLine={false}
                                    width={80}
                                />
                                <ChartTooltip
                                    content={
                                        <ChartTooltipContent
                                            formatter={(value, name) => (
                                                <div className="flex items-center justify-between gap-8">
                                                    <span>{name}</span>
                                                    <span className="font-mono font-medium">
                                                        {formatFullCurrency(value as number)}
                                                    </span>
                                                </div>
                                            )}
                                        />
                                    }
                                />
                                <ChartLegend content={<ChartLegendContent />} />
                                {activeAccountTypes.includes("Taxable") && (
                                    <Bar
                                        dataKey="Taxable"
                                        stackId="a"
                                        fill="var(--color-Taxable)"
                                        radius={[0, 0, 0, 0]}
                                    />
                                )}
                                {activeAccountTypes.includes("TaxDeferred") && (
                                    <Bar
                                        dataKey="TaxDeferred"
                                        stackId="a"
                                        fill="var(--color-TaxDeferred)"
                                        radius={[0, 0, 0, 0]}
                                    />
                                )}
                                {activeAccountTypes.includes("TaxFree") && (
                                    <Bar
                                        dataKey="TaxFree"
                                        stackId="a"
                                        fill="var(--color-TaxFree)"
                                        radius={[4, 4, 0, 0]}
                                    />
                                )}
                                {activeAccountTypes.includes("Illiquid") && (
                                    <Bar
                                        dataKey="Illiquid"
                                        stackId="a"
                                        fill="var(--color-Illiquid)"
                                        radius={[4, 4, 0, 0]}
                                    />
                                )}
                                {activeAccountTypes.includes("Debt") && (
                                    <Bar
                                        dataKey="Debt"
                                        stackId="a"
                                        fill="var(--color-Debt)"
                                        radius={[0, 0, 4, 4]}
                                    />
                                )}
                            </BarChart>
                        </ChartContainer>
                    </CardContent>
                </Card>
            )}

            {/* Portfolio Growth Components */}
            {growthComponentsData.length > 0 && (
                <Card>
                    <CardHeader>
                        <CardTitle>Portfolio Growth Components</CardTitle>
                        <CardDescription>
                            Breakdown of portfolio value by source: initial principal, cumulative contributions, investment returns, and withdrawals.
                        </CardDescription>
                    </CardHeader>
                    <CardContent>
                        <ChartContainer config={growthComponentsConfig} className="h-[400px] w-full">
                            <BarChart
                                accessibilityLayer
                                data={growthComponentsData}
                                margin={{ top: 20, right: 30, left: 0, bottom: 5 }}
                                stackOffset="sign"
                            >
                                <CartesianGrid vertical={false} />
                                <XAxis
                                    dataKey="year"
                                    tickLine={false}
                                    tickMargin={10}
                                    axisLine={false}
                                />
                                <YAxis
                                    tickFormatter={(value) => formatCurrency(value)}
                                    tickLine={false}
                                    axisLine={false}
                                    width={80}
                                />
                                <ChartTooltip
                                    content={
                                        <ChartTooltipContent
                                            formatter={(value, name) => (
                                                <div className="flex items-center justify-between gap-8">
                                                    <span>{name}</span>
                                                    <span className="font-mono font-medium">
                                                        {formatFullCurrency(value as number)}
                                                    </span>
                                                </div>
                                            )}
                                        />
                                    }
                                />
                                <ChartLegend content={<ChartLegendContent />} />
                                {activeGrowthComponents.includes("principal") && (
                                    <Bar
                                        dataKey="principal"
                                        stackId="a"
                                        fill="var(--color-principal)"
                                        radius={[0, 0, 0, 0]}
                                    />
                                )}
                                {activeGrowthComponents.includes("contributions") && (
                                    <Bar
                                        dataKey="contributions"
                                        stackId="a"
                                        fill="var(--color-contributions)"
                                        radius={[0, 0, 0, 0]}
                                    />
                                )}
                                {activeGrowthComponents.includes("returns") && (
                                    <Bar
                                        dataKey="returns"
                                        stackId="a"
                                        fill="var(--color-returns)"
                                        radius={[4, 4, 0, 0]}
                                    />
                                )}
                                {activeGrowthComponents.includes("withdrawals") && (
                                    <Bar
                                        dataKey="withdrawals"
                                        stackId="a"
                                        fill="var(--color-withdrawals)"
                                        radius={[0, 0, 4, 4]}
                                    />
                                )}
                            </BarChart>
                        </ChartContainer>
                    </CardContent>
                </Card>
            )}

            {/* Detailed Data */}
            <Tabs defaultValue="portfolio">
                <TabsList>
                    <TabsTrigger value="portfolio">Total Portfolio</TabsTrigger>
                    {accountIds.map((id) => (
                        <TabsTrigger key={id} value={`account-${id}`}>
                            Account {id}
                        </TabsTrigger>
                    ))}
                </TabsList>

                <TabsContent value="portfolio">
                    <Card>
                        <CardHeader>
                            <CardTitle>Portfolio Values by Year</CardTitle>
                            <CardDescription>
                                Detailed breakdown of projected values at each year
                            </CardDescription>
                        </CardHeader>
                        <CardContent>
                            <DataTable data={result.total_portfolio} formatCurrency={formatFullCurrency} />
                        </CardContent>
                    </Card>
                </TabsContent>

                {accountIds.map((id) => (
                    <TabsContent key={id} value={`account-${id}`}>
                        <Card>
                            <CardHeader>
                                <CardTitle>Account {id} Values by Year</CardTitle>
                                <CardDescription>
                                    Detailed breakdown for this specific account
                                </CardDescription>
                            </CardHeader>
                            <CardContent>
                                <DataTable
                                    data={result.accounts[id] || []}
                                    formatCurrency={formatFullCurrency}
                                />
                            </CardContent>
                        </Card>
                    </TabsContent>
                ))}
            </Tabs>
        </div>
    );
}

function DataTable({
    data,
    formatCurrency,
}: {
    data: TimePointStats[];
    formatCurrency: (v: number) => string;
}) {
    // Sample data to show reasonable rows (every year or so)
    const sampledData = data.filter((_, index) => index % 12 === 0 || index === data.length - 1);

    return (
        <div className="max-h-[400px] overflow-auto">
            <Table>
                <TableHeader>
                    <TableRow>
                        <TableHead>Date</TableHead>
                        <TableHead className="text-right">10th Percentile</TableHead>
                        <TableHead className="text-right">Median</TableHead>
                        <TableHead className="text-right">90th Percentile</TableHead>
                    </TableRow>
                </TableHeader>
                <TableBody>
                    {sampledData.map((point) => (
                        <TableRow key={point.date}>
                            <TableCell className="font-medium">
                                {new Date(point.date).toLocaleDateString("en-US", {
                                    year: "numeric",
                                    month: "short",
                                })}
                            </TableCell>
                            <TableCell className="text-right">{formatCurrency(point.p10)}</TableCell>
                            <TableCell className="text-right font-medium">{formatCurrency(point.p50)}</TableCell>
                            <TableCell className="text-right">{formatCurrency(point.p90)}</TableCell>
                        </TableRow>
                    ))}
                </TableBody>
            </Table>
        </div>
    );
}
