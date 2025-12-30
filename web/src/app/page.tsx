"use client";

import { useState } from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { TabsList, TabsTrigger, TabsContent } from "@/components/ui/tabs";
import { PortfolioEditor } from "@/components/portfolio-editor";
import { SimulationParameters } from "@/components/simulation-parameters";
import { SimulationResults } from "@/components/simulation-results";
import { SimulationParameters as SimParams, AggregatedResult } from "@/types";

const defaultParams: SimParams = {
    duration_years: 30,
    inflation_profile: { Fixed: 0.03 },
    events: [],
    accounts: [
        {
            account_id: 1,
            name: "Savings",
            initial_balance: 10000,
            account_type: "Taxable",
            return_profile: {
                Normal: {
                    mean: 0.079088,
                    std_dev: 0.161832,
                },
            },
            cash_flows: [],
        },
    ],
};

export default function Home() {
    const [activeTab, setActiveTab] = useState("portfolio");
    const [params, setParams] = useState<SimParams>(defaultParams);
    const [result, setResult] = useState<AggregatedResult | null>(null);
    const [loading, setLoading] = useState(false);

    const runSimulation = async () => {
        setLoading(true);
        try {
            const response = await fetch("http://localhost:3000/api/simulate", {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                },
                body: JSON.stringify(params),
            });
            const data = await response.json();
            setResult(data);
            setActiveTab("results");
        } catch (e) {
            console.error(e);
        } finally {
            setLoading(false);
        }
    };

    return (
        <main className="min-h-screen bg-gradient-to-br from-slate-50 to-slate-100 p-8">
            <div className="max-w-7xl mx-auto">
                <div className="mb-8">
                    <h1 className="text-4xl font-bold text-slate-900 mb-2">
                        FinPlan
                    </h1>
                    <p className="text-slate-600">
                        Monte Carlo Financial Planning & Portfolio Simulation
                    </p>
                </div>

                <Card>
                    <CardHeader>
                        <TabsList className="grid w-full grid-cols-3">
                            <TabsTrigger
                                isActive={activeTab === "portfolio"}
                                onClick={() => setActiveTab("portfolio")}
                            >
                                Portfolio
                            </TabsTrigger>
                            <TabsTrigger
                                isActive={activeTab === "parameters"}
                                onClick={() => setActiveTab("parameters")}
                            >
                                Parameters
                            </TabsTrigger>
                            <TabsTrigger
                                isActive={activeTab === "results"}
                                onClick={() => setActiveTab("results")}
                            >
                                Results
                            </TabsTrigger>
                        </TabsList>
                    </CardHeader>
                    <CardContent>
                        {activeTab === "portfolio" && (
                            <TabsContent>
                                <PortfolioEditor params={params} setParams={setParams} />
                            </TabsContent>
                        )}
                        {activeTab === "parameters" && (
                            <TabsContent>
                                <SimulationParameters
                                    params={params}
                                    setParams={setParams}
                                    onRunSimulation={runSimulation}
                                    loading={loading}
                                />
                            </TabsContent>
                        )}
                        {activeTab === "results" && (
                            <TabsContent>
                                <SimulationResults result={result} loading={loading} />
                            </TabsContent>
                        )}
                    </CardContent>
                </Card>
            </div>
        </main>
    );
}
