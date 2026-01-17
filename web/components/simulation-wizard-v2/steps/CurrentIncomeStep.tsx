"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Plus, Trash2 } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";
import { PayFrequency } from "../types";

const PAY_FREQUENCIES: { value: PayFrequency; label: string }[] = [
    { value: "Weekly", label: "Weekly" },
    { value: "BiWeekly", label: "Every two weeks (bi-weekly)" },
    { value: "SemiMonthly", label: "Twice a month (semi-monthly)" },
    { value: "Monthly", label: "Monthly" },
];

export function CurrentIncomeStep() {
    const income = useWizardStore((state) => state.income);
    const setIncome = useWizardStore((state) => state.setIncome);
    const addOtherIncome = useWizardStore((state) => state.addOtherIncome);
    const updateOtherIncome = useWizardStore((state) => state.updateOtherIncome);
    const removeOtherIncome = useWizardStore((state) => state.removeOtherIncome);

    const [editingIncomeId, setEditingIncomeId] = React.useState<string | null>(null);

    const [showEmployer401k, setShowEmployer401k] = React.useState(false);
    const [has401kMatch, setHas401kMatch] = React.useState<string | null>(null);

    const employerMatchAmount = React.useMemo(() => {
        if (!income.employer401k || income.salary === 0) return 0;
        const { matchPercentage, matchUpTo } = income.employer401k;
        const maxEmployerContribution = (income.salary * matchUpTo) / 100;
        return maxEmployerContribution;
    }, [income.employer401k, income.salary]);

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            useGrouping: true,
            maximumFractionDigits: 0,

        }).format(amount);

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Let's talk about your income</h2>
                <p className="text-muted-foreground mt-2">
                    Understanding your income helps us model your cash flow and retirement savings potential.
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>Do you currently have earned income?</CardTitle>
                </CardHeader>
                <CardContent>
                    <RadioGroup
                        value={income.employed ? "employed" : "not-employed"}
                        onValueChange={(value) => setIncome({ employed: value === "employed" })}
                    >
                        <div className="space-y-2">
                            <div className="flex items-center space-x-3">
                                <RadioGroupItem value="employed" id="employed" />
                                <Label htmlFor="employed" className="cursor-pointer">
                                    Yes, I'm employed
                                </Label>
                            </div>
                            <div className="flex items-center space-x-3">
                                <RadioGroupItem value="not-employed" id="not-employed" />
                                <Label htmlFor="not-employed" className="cursor-pointer">
                                    No, I'm not currently working
                                </Label>
                            </div>
                        </div>
                    </RadioGroup>
                </CardContent>
            </Card>

            {income.employed && (
                <>
                    <Card>
                        <CardHeader>
                            <CardTitle>What's your gross annual salary?</CardTitle>
                            <CardDescription>
                                Enter your salary before taxes and deductions
                            </CardDescription>
                        </CardHeader>
                        <CardContent className="space-y-4">
                            <div className="space-y-2">
                                <Label htmlFor="salary">Annual Salary</Label>
                                <MoneyInput
                                    value={income.salary}
                                    onChange={(value) => setIncome({ salary: value })}
                                    placeholder="125000"
                                    showHelper
                                    helperFrequency="month"
                                />
                            </div>
                        </CardContent>
                    </Card>

                    {income.salary > 0 && (
                        <>
                            <Card>
                                <CardHeader>
                                    <CardTitle>How often are you paid?</CardTitle>
                                </CardHeader>
                                <CardContent>
                                    <RadioGroup
                                        value={income.payFrequency}
                                        onValueChange={(value) => setIncome({ payFrequency: value as PayFrequency })}
                                    >
                                        <div className="space-y-2">
                                            {PAY_FREQUENCIES.map((freq) => (
                                                <div key={freq.value} className="flex items-center space-x-3">
                                                    <RadioGroupItem value={freq.value} id={freq.value} />
                                                    <Label htmlFor={freq.value} className="cursor-pointer">
                                                        {freq.label}
                                                    </Label>
                                                </div>
                                            ))}
                                        </div>
                                    </RadioGroup>
                                </CardContent>
                            </Card>

                            <Card>
                                <CardHeader>
                                    <CardTitle>Does your employer offer a 401(k)?</CardTitle>
                                </CardHeader>
                                <CardContent className="space-y-4">
                                    <RadioGroup
                                        value={showEmployer401k ? "yes" : "no"}
                                        onValueChange={(value) => {
                                            setShowEmployer401k(value === "yes");
                                            if (value === "no") {
                                                setIncome({ employer401k: null });
                                                setHas401kMatch(null);
                                            }
                                        }}
                                    >
                                        <div className="space-y-2">
                                            <div className="flex items-center space-x-3">
                                                <RadioGroupItem value="yes" id="has-401k" />
                                                <Label htmlFor="has-401k" className="cursor-pointer">
                                                    Yes, my employer offers a 401(k)
                                                </Label>
                                            </div>
                                            <div className="flex items-center space-x-3">
                                                <RadioGroupItem value="no" id="no-401k" />
                                                <Label htmlFor="no-401k" className="cursor-pointer">
                                                    No or I'm not sure
                                                </Label>
                                            </div>
                                        </div>
                                    </RadioGroup>

                                    {showEmployer401k && (
                                        <div className="space-y-4 pt-4 border-t">
                                            <div>
                                                <Label className="text-base">Do they offer a match?</Label>
                                                <RadioGroup
                                                    value={has401kMatch || ""}
                                                    onValueChange={(value) => {
                                                        setHas401kMatch(value);
                                                        if (value === "no") {
                                                            setIncome({
                                                                employer401k: {
                                                                    hasMatch: false,
                                                                    matchPercentage: 0,
                                                                    matchUpTo: 0,
                                                                    employeeContribution: 0,
                                                                },
                                                            });
                                                        }
                                                    }}
                                                    className="mt-2"
                                                >
                                                    <div className="space-y-2">
                                                        <div className="flex items-center space-x-3">
                                                            <RadioGroupItem value="yes" id="has-match" />
                                                            <Label htmlFor="has-match" className="cursor-pointer">
                                                                Yes, with employer match
                                                            </Label>
                                                        </div>
                                                        <div className="flex items-center space-x-3">
                                                            <RadioGroupItem value="no" id="no-match" />
                                                            <Label htmlFor="no-match" className="cursor-pointer">
                                                                No match offered
                                                            </Label>
                                                        </div>
                                                    </div>
                                                </RadioGroup>
                                            </div>

                                            {has401kMatch === "yes" && (
                                                <div className="space-y-4 pl-4 border-l-2">
                                                    <div className="grid grid-cols-2 gap-4">
                                                        <div className="space-y-2">
                                                            <Label htmlFor="match-pct">Match Percentage</Label>
                                                            <div className="relative">
                                                                <Input
                                                                    id="match-pct"
                                                                    type="number"
                                                                    value={income.employer401k?.matchPercentage || 0}
                                                                    onChange={(e) =>
                                                                        setIncome({
                                                                            employer401k: {
                                                                                ...income.employer401k!,
                                                                                hasMatch: true,
                                                                                matchPercentage: parseFloat(e.target.value) || 0,
                                                                            },
                                                                        })
                                                                    }
                                                                    placeholder="50"
                                                                    min="0"
                                                                    max="100"
                                                                />
                                                                <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                                                                    %
                                                                </span>
                                                            </div>
                                                        </div>
                                                        <div className="space-y-2">
                                                            <Label htmlFor="match-up-to">Up to % of Salary</Label>
                                                            <div className="relative">
                                                                <Input
                                                                    id="match-up-to"
                                                                    type="number"
                                                                    value={income.employer401k?.matchUpTo || 0}
                                                                    onChange={(e) =>
                                                                        setIncome({
                                                                            employer401k: {
                                                                                ...income.employer401k!,
                                                                                hasMatch: true,
                                                                                matchUpTo: parseFloat(e.target.value) || 0,
                                                                            },
                                                                        })
                                                                    }
                                                                    placeholder="6"
                                                                    min="0"
                                                                    max="100"
                                                                />
                                                                <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                                                                    %
                                                                </span>
                                                            </div>
                                                        </div>
                                                    </div>

                                                    {income.employer401k && employerMatchAmount > 0 && (
                                                        <div className="rounded-lg bg-muted p-3 text-sm">
                                                            <p>
                                                                Your employer will contribute up to{" "}
                                                                <span className="font-medium">{formatCurrency(employerMatchAmount)}/year</span>{" "}
                                                                if you contribute{" "}
                                                                <span className="font-medium">
                                                                    {formatCurrency((income.salary * income.employer401k.matchUpTo) / 100)}
                                                                </span>
                                                                .
                                                            </p>
                                                        </div>
                                                    )}
                                                </div>
                                            )}

                                            {(has401kMatch === "yes" || has401kMatch === "no") && (
                                                <div className="space-y-2">
                                                    <Label htmlFor="employee-contribution">
                                                        Are you currently contributing?
                                                    </Label>
                                                    <div className="flex items-center gap-4">
                                                        <div className="relative flex-1">
                                                            <Input
                                                                id="employee-contribution"
                                                                type="number"
                                                                value={income.employer401k?.employeeContribution || 0}
                                                                onChange={(e) =>
                                                                    setIncome({
                                                                        employer401k: {
                                                                            hasMatch: has401kMatch === "yes",
                                                                            matchPercentage: income.employer401k?.matchPercentage || 0,
                                                                            matchUpTo: income.employer401k?.matchUpTo || 0,
                                                                            employeeContribution: parseFloat(e.target.value) || 0,
                                                                        },
                                                                    })
                                                                }
                                                                placeholder="10"
                                                                min="0"
                                                                max="100"
                                                            />
                                                            <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                                                                %
                                                            </span>
                                                        </div>
                                                        {income.employer401k && income.employer401k.employeeContribution > 0 && (
                                                            <span className="text-sm text-muted-foreground whitespace-nowrap">
                                                                ({formatCurrency((income.salary * income.employer401k.employeeContribution) / 100)}/year)
                                                            </span>
                                                        )}
                                                    </div>

                                                    {income.employer401k && income.employer401k.employeeContribution > 0 && (
                                                        <div>
                                                            {income.employer401k.employeeContribution * income.salary / 100 > 23500 && (
                                                                <p className="text-xs text-orange-600 mt-1">
                                                                    ⚠️ This exceeds the 2026 IRS limit of $23,500
                                                                </p>
                                                            )}
                                                        </div>
                                                    )}
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </CardContent>
                            </Card>
                        </>
                    )}

                    <Card>
                        <CardHeader>
                            <CardTitle>Do you have any other income sources?</CardTitle>
                            <CardDescription>
                                Side business, rental income, alimony, pension, etc.
                            </CardDescription>
                        </CardHeader>
                        <CardContent className="space-y-4">
                            {income.otherIncome.length === 0 ? (
                                <p className="text-sm text-muted-foreground">No additional income sources added yet</p>
                            ) : (
                                <div className="space-y-3">
                                    {income.otherIncome.map((source) => (
                                        <div
                                            key={source.id}
                                            className="border rounded-lg p-4 space-y-3"
                                        >
                                            <div className="flex items-start justify-between">
                                                <div className="flex-1 space-y-3">
                                                    <div className="space-y-2">
                                                        <Label htmlFor={`desc-${source.id}`}>Description</Label>
                                                        <Input
                                                            id={`desc-${source.id}`}
                                                            value={source.description}
                                                            onChange={(e) => updateOtherIncome(source.id, { description: e.target.value })}
                                                            placeholder="e.g., Side business, Rental income"
                                                        />
                                                    </div>
                                                    <div className="grid grid-cols-2 gap-4">
                                                        <div className="space-y-2">
                                                            <Label htmlFor={`amount-${source.id}`}>Amount</Label>
                                                            <MoneyInput
                                                                value={source.amount}
                                                                onChange={(value) => updateOtherIncome(source.id, { amount: value })}
                                                                placeholder="5000"
                                                            />
                                                        </div>
                                                        <div className="space-y-2">
                                                            <Label htmlFor={`freq-${source.id}`}>Frequency</Label>
                                                            <select
                                                                id={`freq-${source.id}`}
                                                                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-base shadow-sm transition-colors file:border-0 file:bg-transparent file:text-sm file:font-medium file:text-foreground placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 md:text-sm"
                                                                value={source.frequency}
                                                                onChange={(e) => updateOtherIncome(source.id, { frequency: e.target.value as PayFrequency })}
                                                            >
                                                                {PAY_FREQUENCIES.map((freq) => (
                                                                    <option key={freq.value} value={freq.value}>
                                                                        {freq.label}
                                                                    </option>
                                                                ))}
                                                            </select>
                                                        </div>
                                                    </div>
                                                </div>
                                                <Button
                                                    variant="ghost"
                                                    size="icon"
                                                    onClick={() => removeOtherIncome(source.id)}
                                                    className="ml-2"
                                                >
                                                    <Trash2 className="h-4 w-4" />
                                                </Button>
                                            </div>
                                        </div>
                                    ))}
                                </div>
                            )}

                            <Button
                                variant="outline"
                                className="w-full"
                                onClick={() => {
                                    const id = `income-${Date.now()}`;
                                    addOtherIncome({
                                        id,
                                        description: "Other Income",
                                        amount: 0,
                                        frequency: "Monthly",
                                    });
                                }}
                            >
                                <Plus className="h-4 w-4 mr-2" />
                                Add Income Source
                            </Button>
                        </CardContent>
                    </Card>
                </>
            )}
        </div>
    );
}
