"use client";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Play } from "lucide-react";
import type { SimulationParameters as SimParams } from "@/types";

interface SimulationParametersProps {
    params: SimParams;
    setParams: (params: SimParams) => void;
    onRunSimulation: () => void;
    loading: boolean;
}

export function SimulationParameters({
    params,
    setParams,
    onRunSimulation,
    loading,
}: SimulationParametersProps) {
    const updateInflationProfile = (type: string, value?: number) => {
        if (type === "None") {
            setParams({ ...params, inflation_profile: "None" });
        } else if (type === "Fixed") {
            setParams({ ...params, inflation_profile: { Fixed: value || 0.03 } });
        } else if (type === "Normal") {
            setParams({
                ...params,
                inflation_profile: { Normal: { mean: 0.03, std_dev: 0.01 } },
            });
        } else {
            setParams({
                ...params,
                inflation_profile: { LogNormal: { mean: 0.03, std_dev: 0.01 } },
            });
        }
    };

    return (
        <div className="space-y-6">
            <div>
                <h2 className="text-2xl font-semibold mb-2">Simulation Parameters</h2>
                <p className="text-sm text-muted-foreground">
                    Configure Monte Carlo simulation settings
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>General Settings</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid grid-cols-2 gap-4">
                        <div>
                            <Label>Duration (Years)</Label>
                            <Input
                                type="number"
                                value={params.duration_years}
                                onChange={(e) =>
                                    setParams({
                                        ...params,
                                        duration_years: parseInt(e.target.value) || 30,
                                    })
                                }
                            />
                        </div>
                        <div>
                            <Label>Start Date (Optional)</Label>
                            <Input
                                type="date"
                                value={params.start_date || ""}
                                onChange={(e) =>
                                    setParams({ ...params, start_date: e.target.value })
                                }
                            />
                        </div>
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Inflation Profile</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid grid-cols-2 gap-4">
                        <div>
                            <Label>Profile Type</Label>
                            <Select
                                value={
                                    typeof params.inflation_profile === "string"
                                        ? "None"
                                        : "Fixed" in params.inflation_profile
                                            ? "Fixed"
                                            : "Normal" in params.inflation_profile
                                                ? "Normal"
                                                : "LogNormal"
                                }
                                onChange={(e) => updateInflationProfile(e.target.value)}
                            >
                                <option value="None">None</option>
                                <option value="Fixed">Fixed</option>
                                <option value="Normal">Normal Distribution</option>
                                <option value="LogNormal">Log Normal Distribution</option>
                            </Select>
                        </div>
                        {typeof params.inflation_profile !== "string" && (
                            <>
                                {"Fixed" in params.inflation_profile ? (
                                    <div>
                                        <Label>Rate</Label>
                                        <Input
                                            type="number"
                                            step="0.001"
                                            value={params.inflation_profile.Fixed}
                                            onChange={(e) =>
                                                updateInflationProfile(
                                                    "Fixed",
                                                    parseFloat(e.target.value) || 0
                                                )
                                            }
                                        />
                                    </div>
                                ) : (
                                    <>
                                        <div>
                                            <Label>Mean</Label>
                                            <Input
                                                type="number"
                                                step="0.001"
                                                value={
                                                    typeof params.inflation_profile !== "string" && "Normal" in params.inflation_profile
                                                        ? params.inflation_profile.Normal.mean
                                                        : typeof params.inflation_profile !== "string" && "LogNormal" in params.inflation_profile
                                                            ? params.inflation_profile.LogNormal.mean
                                                            : 0
                                                }
                                                onChange={(e) => {
                                                    const mean = parseFloat(e.target.value) || 0;
                                                    if (typeof params.inflation_profile !== "string") {
                                                        const profile =
                                                            "Normal" in params.inflation_profile
                                                                ? {
                                                                    Normal: {
                                                                        ...params.inflation_profile.Normal,
                                                                        mean,
                                                                    },
                                                                }
                                                                : "LogNormal" in params.inflation_profile
                                                                    ? {
                                                                        LogNormal: {
                                                                            ...params.inflation_profile.LogNormal,
                                                                            mean,
                                                                        },
                                                                    }
                                                                    : params.inflation_profile;
                                                        setParams({ ...params, inflation_profile: profile });
                                                    }
                                                }}
                                            />
                                        </div>
                                        <div>
                                            <Label>Standard Deviation</Label>
                                            <Input
                                                type="number"
                                                step="0.001"
                                                value={
                                                    typeof params.inflation_profile !== "string" && "Normal" in params.inflation_profile
                                                        ? params.inflation_profile.Normal.std_dev
                                                        : typeof params.inflation_profile !== "string" && "LogNormal" in params.inflation_profile
                                                            ? params.inflation_profile.LogNormal.std_dev
                                                            : 0
                                                }
                                                onChange={(e) => {
                                                    const std_dev = parseFloat(e.target.value) || 0;
                                                    if (typeof params.inflation_profile !== "string") {
                                                        const profile =
                                                            "Normal" in params.inflation_profile
                                                                ? {
                                                                    Normal: {
                                                                        ...params.inflation_profile.Normal,
                                                                        std_dev,
                                                                    },
                                                                }
                                                                : "LogNormal" in params.inflation_profile
                                                                    ? {
                                                                        LogNormal: {
                                                                            ...params.inflation_profile.LogNormal,
                                                                            std_dev,
                                                                        },
                                                                    }
                                                                    : params.inflation_profile;
                                                        setParams({ ...params, inflation_profile: profile });
                                                    }
                                                }}
                                            />
                                        </div>
                                    </>
                                )}
                            </>
                        )}
                    </div>
                </CardContent>
            </Card>

            <div className="flex justify-end">
                <Button onClick={onRunSimulation} disabled={loading} size="lg">
                    <Play className="mr-2 h-5 w-5" />
                    {loading ? "Running Simulation..." : "Run Monte Carlo Simulation"}
                </Button>
            </div>
        </div>
    );
}
