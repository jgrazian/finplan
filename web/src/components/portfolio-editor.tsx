"use client";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Plus, Trash2 } from "lucide-react";
import { SimulationParameters, Account } from "@/types";

interface PortfolioEditorProps {
    params: SimulationParameters;
    setParams: (params: SimulationParameters) => void;
}

export function PortfolioEditor({ params, setParams }: PortfolioEditorProps) {
    const addAccount = () => {
        const newAccount: Account = {
            account_id: Math.max(0, ...params.accounts.map((a) => a.account_id)) + 1,
            name: "New Account",
            initial_balance: 0,
            account_type: "Taxable",
            return_profile: { Fixed: 0.07 },
            cash_flows: [],
        };
        setParams({ ...params, accounts: [...params.accounts, newAccount] });
    };

    const updateAccount = (index: number, updates: Partial<Account>) => {
        const newAccounts = [...params.accounts];
        newAccounts[index] = { ...newAccounts[index], ...updates };
        setParams({ ...params, accounts: newAccounts });
    };

    const deleteAccount = (index: number) => {
        setParams({
            ...params,
            accounts: params.accounts.filter((_, i) => i !== index),
        });
    };

    const addCashFlow = (accountIndex: number) => {
        const account = params.accounts[accountIndex];
        const newCashFlow = {
            cash_flow_id:
                Math.max(0, ...account.cash_flows.map((cf) => cf.cash_flow_id)) + 1,
            description: "New Cash Flow",
            amount: 0,
            start: "Immediate" as const,
            end: "Never" as const,
            repeats: "Monthly" as const,
            adjust_for_inflation: false,
        };
        updateAccount(accountIndex, {
            cash_flows: [...account.cash_flows, newCashFlow],
        });
    };

    const updateCashFlow = (
        accountIndex: number,
        cfIndex: number,
        updates: any
    ) => {
        const account = params.accounts[accountIndex];
        const newCashFlows = [...account.cash_flows];
        newCashFlows[cfIndex] = { ...newCashFlows[cfIndex], ...updates };
        updateAccount(accountIndex, { cash_flows: newCashFlows });
    };

    const deleteCashFlow = (accountIndex: number, cfIndex: number) => {
        const account = params.accounts[accountIndex];
        updateAccount(accountIndex, {
            cash_flows: account.cash_flows.filter((_, i) => i !== cfIndex),
        });
    };

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <div>
                    <h2 className="text-2xl font-semibold">Portfolio Configuration</h2>
                    <p className="text-sm text-muted-foreground">
                        Configure your accounts, assets, and cash flows
                    </p>
                </div>
                <Button onClick={addAccount}>
                    <Plus className="mr-2 h-4 w-4" />
                    Add Account
                </Button>
            </div>

            {params.accounts.map((account, accountIndex) => (
                <Card key={account.account_id}>
                    <CardHeader>
                        <div className="flex justify-between items-start">
                            <div className="flex-1 grid grid-cols-2 gap-4">
                                <div>
                                    <Label>Account Name</Label>
                                    <Input
                                        value={account.name}
                                        onChange={(e) =>
                                            updateAccount(accountIndex, { name: e.target.value })
                                        }
                                    />
                                </div>
                                <div>
                                    <Label>Account Type</Label>
                                    <Select
                                        value={account.account_type}
                                        onChange={(e) =>
                                            updateAccount(accountIndex, {
                                                account_type: e.target.value as any,
                                            })
                                        }
                                    >
                                        <option value="Taxable">Taxable</option>
                                        <option value="TaxDeferred">Tax Deferred</option>
                                        <option value="TaxFree">Tax Free</option>
                                        <option value="Liability">Liability</option>
                                    </Select>
                                </div>
                                <div>
                                    <Label>Initial Balance ($)</Label>
                                    <Input
                                        type="number"
                                        value={account.initial_balance}
                                        onChange={(e) =>
                                            updateAccount(accountIndex, {
                                                initial_balance: parseFloat(e.target.value) || 0,
                                            })
                                        }
                                    />
                                </div>
                                <div>
                                    <Label>Return Profile</Label>
                                    <div className="flex gap-2">
                                        <Select
                                            value={
                                                typeof account.return_profile === "string"
                                                    ? "None"
                                                    : "Fixed" in account.return_profile
                                                        ? "Fixed"
                                                        : "Normal" in account.return_profile
                                                            ? "Normal"
                                                            : "LogNormal"
                                            }
                                            onChange={(e) => {
                                                const type = e.target.value;
                                                if (type === "None") {
                                                    updateAccount(accountIndex, {
                                                        return_profile: "None",
                                                    });
                                                } else if (type === "Fixed") {
                                                    updateAccount(accountIndex, {
                                                        return_profile: { Fixed: 0.07 },
                                                    });
                                                } else if (type === "Normal") {
                                                    updateAccount(accountIndex, {
                                                        return_profile: {
                                                            Normal: { mean: 0.07, std_dev: 0.15 },
                                                        },
                                                    });
                                                } else {
                                                    updateAccount(accountIndex, {
                                                        return_profile: {
                                                            LogNormal: { mean: 0.07, std_dev: 0.15 },
                                                        },
                                                    });
                                                }
                                            }}
                                        >
                                            <option value="None">None</option>
                                            <option value="Fixed">Fixed</option>
                                            <option value="Normal">Normal</option>
                                            <option value="LogNormal">Log Normal</option>
                                        </Select>
                                        {typeof account.return_profile !== "string" &&
                                            "Fixed" in account.return_profile && (
                                                <Input
                                                    type="number"
                                                    step="0.001"
                                                    value={account.return_profile.Fixed}
                                                    onChange={(e) =>
                                                        updateAccount(accountIndex, {
                                                            return_profile: {
                                                                Fixed: parseFloat(e.target.value) || 0,
                                                            },
                                                        })
                                                    }
                                                    placeholder="Rate"
                                                />
                                            )}
                                    </div>
                                </div>
                            </div>
                            <Button
                                variant="destructive"
                                size="icon"
                                onClick={() => deleteAccount(accountIndex)}
                            >
                                <Trash2 className="h-4 w-4" />
                            </Button>
                        </div>
                    </CardHeader>
                    <CardContent>
                        <div className="space-y-4">
                            <div className="flex justify-between items-center">
                                <h3 className="text-lg font-semibold">Cash Flows</h3>
                                <Button
                                    variant="outline"
                                    size="sm"
                                    onClick={() => addCashFlow(accountIndex)}
                                >
                                    <Plus className="mr-2 h-4 w-4" />
                                    Add Cash Flow
                                </Button>
                            </div>

                            {account.cash_flows.map((cf, cfIndex) => (
                                <Card key={cf.cash_flow_id} className="bg-muted/50">
                                    <CardContent className="pt-6">
                                        <div className="grid grid-cols-4 gap-4">
                                            <div>
                                                <Label>Description</Label>
                                                <Input
                                                    value={cf.description || ""}
                                                    onChange={(e) =>
                                                        updateCashFlow(accountIndex, cfIndex, {
                                                            description: e.target.value,
                                                        })
                                                    }
                                                />
                                            </div>
                                            <div>
                                                <Label>Amount ($)</Label>
                                                <Input
                                                    type="number"
                                                    value={cf.amount}
                                                    onChange={(e) =>
                                                        updateCashFlow(accountIndex, cfIndex, {
                                                            amount: parseFloat(e.target.value) || 0,
                                                        })
                                                    }
                                                />
                                            </div>
                                            <div>
                                                <Label>Frequency</Label>
                                                <Select
                                                    value={cf.repeats}
                                                    onChange={(e) =>
                                                        updateCashFlow(accountIndex, cfIndex, {
                                                            repeats: e.target.value,
                                                        })
                                                    }
                                                >
                                                    <option value="Never">One-time</option>
                                                    <option value="Weekly">Weekly</option>
                                                    <option value="BiWeekly">Bi-Weekly</option>
                                                    <option value="Monthly">Monthly</option>
                                                    <option value="Quarterly">Quarterly</option>
                                                    <option value="Yearly">Yearly</option>
                                                </Select>
                                            </div>
                                            <div className="flex items-end">
                                                <Button
                                                    variant="destructive"
                                                    size="sm"
                                                    onClick={() => deleteCashFlow(accountIndex, cfIndex)}
                                                >
                                                    <Trash2 className="h-4 w-4" />
                                                </Button>
                                            </div>
                                        </div>
                                    </CardContent>
                                </Card>
                            ))}
                        </div>
                    </CardContent>
                </Card>
            ))}
        </div>
    );
}
