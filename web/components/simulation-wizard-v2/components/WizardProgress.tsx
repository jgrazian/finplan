"use client";

import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { CheckCircle2, Circle } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { useCalculations } from "../hooks/useCalculations";
import { cn } from "@/lib/utils";

const STEPS = [
    { id: 0, title: "Welcome", shortTitle: "Welcome" },
    { id: 1, title: "About You", shortTitle: "Personal" },
    { id: 2, title: "Current Income", shortTitle: "Income" },
    { id: 3, title: "Current Savings", shortTitle: "Savings" },
    { id: 4, title: "Investments", shortTitle: "Investments" },
    { id: 5, title: "Real Estate", shortTitle: "Property" },
    { id: 6, title: "Debts", shortTitle: "Debts" },
    { id: 7, title: "Retirement Goals", shortTitle: "Retirement" },
    { id: 8, title: "Life Events", shortTitle: "Events" },
    { id: 9, title: "Review & Refine", shortTitle: "Review" },
];

export function WizardProgress() {
    const currentStep = useWizardStore((state) => state.currentStep);
    const completedSteps = useWizardStore((state) => state.completedSteps);
    const goToStep = useWizardStore((state) => state.goToStep);

    const {
        netWorth,
        monthlyIncome,
        monthlyExpenses,
        currentAge,
        yearsToRetirement,
        formatCurrency,
    } = useCalculations();

    const progressPercentage = ((currentStep + 1) / STEPS.length) * 100;

    return (
        <div className="w-80 border-r bg-muted/40 p-6 space-y-6">
            {/* Step Progress */}
            <Card>
                <CardHeader>
                    <CardTitle className="text-sm">Your Progress</CardTitle>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="space-y-1">
                        <div className="flex justify-between text-xs text-muted-foreground">
                            <span>Step {currentStep + 1} of {STEPS.length}</span>
                            <span>{Math.round(progressPercentage)}%</span>
                        </div>
                        <Progress value={progressPercentage} className="h-2" />
                    </div>

                    <div className="space-y-1">
                        {STEPS.map((step) => {
                            const isCompleted = completedSteps.has(step.id);
                            const isCurrent = currentStep === step.id;
                            const isClickable = isCompleted || step.id <= currentStep;

                            return (
                                <button
                                    key={step.id}
                                    onClick={() => isClickable && goToStep(step.id)}
                                    disabled={!isClickable}
                                    className={cn(
                                        "flex items-center gap-2 w-full px-2 py-1.5 rounded text-sm transition-colors",
                                        isCurrent && "bg-primary text-primary-foreground font-medium",
                                        !isCurrent && isCompleted && "text-muted-foreground hover:bg-muted",
                                        !isCurrent && !isCompleted && "text-muted-foreground/50 cursor-not-allowed"
                                    )}
                                >
                                    {isCompleted ? (
                                        <CheckCircle2 className="h-4 w-4 text-green-600" />
                                    ) : (
                                        <Circle className={cn("h-4 w-4", isCurrent && "fill-current")} />
                                    )}
                                    <span className="flex-1 text-left">{step.shortTitle}</span>
                                </button>
                            );
                        })}
                    </div>
                </CardContent>
            </Card>

            {/* Financial Summary */}
            <Card>
                <CardHeader>
                    <CardTitle className="text-sm">Your Financial Picture</CardTitle>
                </CardHeader>
                <CardContent className="space-y-3">
                    <div className="space-y-2 text-sm">
                        <div className="flex justify-between">
                            <span className="text-muted-foreground">Net Worth:</span>
                            <span className="font-medium">{formatCurrency(netWorth)}</span>
                        </div>

                        {monthlyIncome > 0 && (
                            <div className="flex justify-between">
                                <span className="text-muted-foreground">Monthly Income:</span>
                                <span className="font-medium">{formatCurrency(monthlyIncome)}</span>
                            </div>
                        )}

                        {monthlyExpenses > 0 && (
                            <div className="flex justify-between">
                                <span className="text-muted-foreground">Monthly Expenses:</span>
                                <span className="font-medium">{formatCurrency(monthlyExpenses)}</span>
                            </div>
                        )}

                        {currentAge && (
                            <div className="flex justify-between">
                                <span className="text-muted-foreground">Current Age:</span>
                                <span className="font-medium">{currentAge}</span>
                            </div>
                        )}

                        {yearsToRetirement !== null && (
                            <div className="flex justify-between">
                                <span className="text-muted-foreground">Years to Retirement:</span>
                                <span className="font-medium">{yearsToRetirement}</span>
                            </div>
                        )}
                    </div>
                </CardContent>
            </Card>
        </div>
    );
}
