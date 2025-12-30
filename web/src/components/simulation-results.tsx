"use client";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
    BarChart,
    Bar,
    XAxis,
    YAxis,
    CartesianGrid,
    Tooltip,
    Legend,
    ResponsiveContainer,
} from "recharts";
import { AggregatedResult } from "@/types";
import { Loader2 } from "lucide-react";

interface SimulationResultsProps {
    result: AggregatedResult | null;
    loading: boolean;
}

const currencyFormatter = (value: number) => {
    return new Intl.NumberFormat("en-US", {
        style: "currency",
        currency: "USD",
        minimumFractionDigits: 0,
        maximumFractionDigits: 0,
    }).format(value);
};

export function SimulationResults({
    result,
    loading,
}: SimulationResultsProps) {
    if (loading) {
        return (
            <div className="flex items-center justify-center h-96">
                <Loader2 className="h-8 w-8 animate-spin text-primary" />
            </div>
        );
    }

    if (!result) {
        return (
            <div className="flex items-center justify-center h-96 text-muted-foreground">
                <div className="text-center">
                    <p className="text-lg">No simulation results yet</p>
                    <p className="text-sm">
                        Configure your portfolio and run a simulation to see results
                    </p>
                </div>
            </div>
        );
    }

    // Get account names and prepare data for bar chart
    const accountIds = Object.keys(result.accounts).map(Number);
    const timePoints = result.total_portfolio.length;
    const intervalYears = 5;
    const step = Math.floor((timePoints / (result.total_portfolio.length > 0 ?
        (new Date(result.total_portfolio[result.total_portfolio.length - 1].date).getFullYear() -
            new Date(result.total_portfolio[0].date).getFullYear()) / intervalYears : 1)) || 1);

    // Sample data points every 5 years
    const sampledData = result.total_portfolio
        .filter((_, index) => index % Math.max(1, Math.floor(timePoints / 7)) === 0)
        .map((point, index) => {
            const dataPoint: any = {
                date: new Date(point.date).toLocaleDateString("en-US", {
                    year: "numeric",
                    month: "short",
                }),
            };

            // Add account data for each percentile
            accountIds.forEach((accountId) => {
                const accountData = result.accounts[accountId];
                if (accountData && accountData[index * Math.floor(timePoints / 7)]) {
                    const stats = accountData[index * Math.floor(timePoints / 7)];
                    dataPoint[`account_${accountId}_p50`] = stats.p50;
                }
            });

            return dataPoint;
        });

    // Prepare colors for accounts
    const colors = [
        "hsl(var(--chart-1))",
        "hsl(var(--chart-2))",
        "hsl(var(--chart-3))",
        "hsl(var(--chart-4))",
        "hsl(var(--chart-5))",
    ];

    return (
        <div className="space-y-6">
            <div>
                <h2 className="text-2xl font-semibold mb-2">Simulation Results</h2>
                <p className="text-sm text-muted-foreground">
                    Monte Carlo simulation projection broken down by account
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>Portfolio Projection by Account (Median Values)</CardTitle>
                </CardHeader>
                <CardContent>
                    <ResponsiveContainer width="100%" height={500}>
                        <BarChart data={sampledData}>
                            <CartesianGrid strokeDasharray="3 3" />
                            <XAxis dataKey="date" />
                            <YAxis
                                tickFormatter={(value) => currencyFormatter(value)}
                            />
                            <Tooltip
                                formatter={(value: any) => currencyFormatter(value)}
                                contentStyle={{
                                    backgroundColor: "hsl(var(--card))",
                                    border: "1px solid hsl(var(--border))",
                                    borderRadius: "0.5rem",
                                }}
                            />
                            <Legend />
                            {accountIds.map((accountId, index) => (
                                <Bar
                                    key={accountId}
                                    dataKey={`account_${accountId}_p50`}
                                    stackId="a"
                                    fill={colors[index % colors.length]}
                                    name={`Account ${accountId}`}
                                />
                            ))}
                        </BarChart>
                    </ResponsiveContainer>
                </CardContent>
            </Card>

            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                <Card>
                    <CardHeader>
                        <CardTitle className="text-sm font-medium">
                            Final Portfolio Value (10th Percentile)
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {currencyFormatter(
                                result.total_portfolio[result.total_portfolio.length - 1].p10
                            )}
                        </div>
                        <p className="text-xs text-muted-foreground mt-1">
                            Worst case scenario (90% chance to exceed)
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader>
                        <CardTitle className="text-sm font-medium">
                            Final Portfolio Value (Median)
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {currencyFormatter(
                                result.total_portfolio[result.total_portfolio.length - 1].p50
                            )}
                        </div>
                        <p className="text-xs text-muted-foreground mt-1">
                            Expected outcome (50% probability)
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader>
                        <CardTitle className="text-sm font-medium">
                            Final Portfolio Value (90th Percentile)
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="text-2xl font-bold">
                            {currencyFormatter(
                                result.total_portfolio[result.total_portfolio.length - 1].p90
                            )}
                        </div>
                        <p className="text-xs text-muted-foreground mt-1">
                            Best case scenario (10% chance to exceed)
                        </p>
                    </CardContent>
                </Card>
            </div>
        </div>
    );
}
