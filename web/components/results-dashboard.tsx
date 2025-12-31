"use client";

import * as React from "react";
import {
    Area,
    AreaChart,
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
import { ChartContainer, ChartTooltipContent } from "@/components/ui/chart";
import { AggregatedResult, TimePointStats } from "@/lib/types";

interface ResultsDashboardProps {
    result: AggregatedResult;
    simulationName?: string;
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

export function ResultsDashboard({ result, simulationName }: ResultsDashboardProps) {
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
