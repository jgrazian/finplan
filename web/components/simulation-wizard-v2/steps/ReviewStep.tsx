"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Loader2, Rocket, Save, AlertTriangle, CheckCircle2, Info } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { useCalculations } from "../hooks/useCalculations";
import { buildSimulationParameters } from "../utils/parameterBuilder";
import { useRouter } from "next/navigation";

export function ReviewStep() {
    const router = useRouter();
    const [isSubmitting, setIsSubmitting] = React.useState(false);
    const [error, setError] = React.useState<string | null>(null);

    const state = useWizardStore();
    const calculations = useCalculations();

    const currentAge = React.useMemo(() => {
        if (!state.personalInfo.birthDate) return null;
        const today = new Date();
        const birthDate = new Date(state.personalInfo.birthDate);
        let age = today.getFullYear() - birthDate.getFullYear();
        const monthDiff = today.getMonth() - birthDate.getMonth();
        if (monthDiff < 0 || (monthDiff === 0 && today.getDate() < birthDate.getDate())) {
            age--;
        }
        return age;
    }, [state.personalInfo.birthDate]);

    const yearsToRetirement = state.retirement.targetAge && currentAge
        ? state.retirement.targetAge - currentAge
        : null;

    const monthlyCashFlow = React.useMemo(() => {
        let income = 0;
        let expenses = 0;

        // Income from salary
        if (state.income.employed && state.income.salary > 0) {
            income += state.income.salary / 12 * 0.75; // Rough after-tax estimate
        }

        // Other income sources
        state.income.otherIncome.forEach(source => {
            const multiplier = source.frequency === "Monthly" ? 1 :
                source.frequency === "BiWeekly" ? 26 / 12 :
                    source.frequency === "Weekly" ? 52 / 12 : 1;
            income += source.amount * multiplier;
        });

        // Rental income
        state.realEstate.forEach(property => {
            if (property.rentalIncome) {
                income += property.rentalIncome;
            }
        });

        // Expenses - mortgage payments
        state.realEstate.forEach(property => {
            if (property.mortgage) {
                expenses += property.mortgage.monthlyPayment;
            }
        });

        // Debt payments
        state.debts.forEach(debt => {
            expenses += debt.monthlyPayment;
        });

        // 401k contributions
        if (state.income.employer401k?.employeeContribution) {
            expenses += (state.income.salary * state.income.employer401k.employeeContribution / 100) / 12;
        }

        // Investment contributions
        state.investments.forEach(inv => {
            if (inv.contributions) {
                const multiplier = inv.contributions.frequency === "Monthly" ? 1 :
                    inv.contributions.frequency === "BiWeekly" ? 26 / 12 :
                        inv.contributions.frequency === "Weekly" ? 52 / 12 : 1;
                expenses += inv.contributions.amount * multiplier;
            }
        });

        return { income, expenses, remaining: income - expenses };
    }, [state]);

    const retirementIncomeGap = React.useMemo(() => {
        if (!state.retirement.targetIncome) return null;

        const targetMonthly = state.retirement.targetIncome / 12;
        let guaranteedIncome = 0;

        if (state.retirement.socialSecurity.hasSSI && state.retirement.socialSecurity.estimatedBenefit) {
            guaranteedIncome += state.retirement.socialSecurity.estimatedBenefit;
        }

        if (state.retirement.pension.hasPension && state.retirement.pension.monthlyAmount) {
            guaranteedIncome += state.retirement.pension.monthlyAmount;
        }

        return {
            target: targetMonthly,
            guaranteed: guaranteedIncome,
            gap: targetMonthly - guaranteedIncome,
        };
    }, [state.retirement]);

    const handleRunSimulation = async () => {
        try {
            setIsSubmitting(true);
            setError(null);

            // Build simulation parameters
            const parameters = buildSimulationParameters(state);

            // Submit to API
            const response = await fetch("/api/simulations", {
                method: "POST",
                headers: {
                    "Content-Type": "application/json",
                },
                body: JSON.stringify({
                    name: state.simulationName,
                    description: `${state.goal || "Financial planning"} simulation`,
                    parameters,
                }),
            });

            if (!response.ok) {
                const errorData = await response.json();
                throw new Error(errorData.error || "Failed to create simulation");
            }

            const simulation = await response.json();

            // Navigate to results
            router.push(`/simulations/${simulation.id}`);
        } catch (err) {
            console.error("Error creating simulation:", err);
            setError(err instanceof Error ? err.message : "An error occurred");
        } finally {
            setIsSubmitting(false);
        }
    };

    const handleSaveDraft = async () => {
        // TODO: Implement draft saving
        console.log("Save draft functionality coming soon");
    };

    return (
        <div className="space-y-6 max-w-4xl pb-12">
            {/* Header */}
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Review & Refine</h2>
                <p className="text-muted-foreground mt-2">
                    Here's your complete financial picture. Review the details before running the simulation.
                </p>
            </div>

            {/* Financial Snapshot */}
            <Card>
                <CardHeader>
                    <CardTitle>Your Financial Snapshot</CardTitle>
                    <CardDescription>Current state of your finances</CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
                        <div>
                            <div className="text-sm text-muted-foreground mb-1">Net Worth Today</div>
                            <div className="text-3xl font-bold">
                                ${calculations.netWorth.toLocaleString()}
                            </div>
                            <div className="text-xs text-muted-foreground mt-2 space-y-1">
                                <div>Liquid Savings: ${calculations.liquidSavings.toLocaleString()}</div>
                                <div>Investment Accounts: ${calculations.totalInvestments.toLocaleString()}</div>
                                <div>Real Estate Equity: ${calculations.realEstateEquity.toLocaleString()}</div>
                                <div className="text-red-600">Total Debts: -${calculations.totalDebts.toLocaleString()}</div>
                            </div>
                        </div>

                        <div>
                            <div className="text-sm text-muted-foreground mb-1">Monthly Cash Flow</div>
                            <div className="space-y-2 text-sm">
                                <div className="flex justify-between">
                                    <span className="text-muted-foreground">Income (after tax):</span>
                                    <span className="font-medium text-green-600">
                                        +${monthlyCashFlow.income.toLocaleString()}
                                    </span>
                                </div>
                                <div className="flex justify-between">
                                    <span className="text-muted-foreground">Housing & Debts:</span>
                                    <span className="font-medium text-red-600">
                                        -${monthlyCashFlow.expenses.toLocaleString()}
                                    </span>
                                </div>
                                <Separator />
                                <div className="flex justify-between font-bold">
                                    <span>Remaining:</span>
                                    <span className={monthlyCashFlow.remaining >= 0 ? "text-green-600" : "text-red-600"}>
                                        ${monthlyCashFlow.remaining.toLocaleString()}
                                    </span>
                                </div>
                            </div>
                        </div>
                    </div>

                    {monthlyCashFlow.remaining < 0 && (
                        <Alert variant="destructive">
                            <AlertTriangle className="h-4 w-4" />
                            <AlertDescription>
                                Your expenses exceed your income by ${Math.abs(monthlyCashFlow.remaining).toLocaleString()}/month.
                                Consider reviewing your budget or increasing income.
                            </AlertDescription>
                        </Alert>
                    )}
                </CardContent>
            </Card>

            {/* Retirement Plan */}
            {state.retirement.targetAge && (
                <Card>
                    <CardHeader>
                        <CardTitle>Retirement Plan</CardTitle>
                        <CardDescription>Your retirement goals and timeline</CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
                            <div>
                                <div className="text-sm text-muted-foreground mb-1">Target Retirement Age</div>
                                <div className="text-2xl font-bold">{state.retirement.targetAge}</div>
                                {yearsToRetirement && (
                                    <div className="text-xs text-muted-foreground mt-1">
                                        {yearsToRetirement} years from now
                                    </div>
                                )}
                            </div>

                            <div>
                                <div className="text-sm text-muted-foreground mb-1">Target Annual Income</div>
                                <div className="text-2xl font-bold">
                                    ${state.retirement.targetIncome?.toLocaleString() || "—"}
                                </div>
                                <div className="text-xs text-muted-foreground mt-1">
                                    ${((state.retirement.targetIncome || 0) / 12).toLocaleString()}/month
                                </div>
                            </div>

                            <div>
                                <div className="text-sm text-muted-foreground mb-1">Current Retirement Savings</div>
                                <div className="text-2xl font-bold">
                                    ${calculations.totalInvestments.toLocaleString()}
                                </div>
                            </div>
                        </div>

                        {retirementIncomeGap && (
                            <>
                                <Separator />
                                <div>
                                    <div className="text-sm font-medium mb-2">Retirement Income Sources</div>
                                    <div className="space-y-2 text-sm">
                                        <div className="flex justify-between">
                                            <span className="text-muted-foreground">Target Monthly Income:</span>
                                            <span className="font-medium">${retirementIncomeGap.target.toLocaleString()}</span>
                                        </div>
                                        {state.retirement.socialSecurity.hasSSI && (
                                            <div className="flex justify-between">
                                                <span className="text-muted-foreground">
                                                    Social Security (at {state.retirement.socialSecurity.claimingAge}):
                                                </span>
                                                <span className="text-green-600">
                                                    +${retirementIncomeGap.guaranteed.toLocaleString()}
                                                </span>
                                            </div>
                                        )}
                                        {state.retirement.pension.hasPension && (
                                            <div className="flex justify-between">
                                                <span className="text-muted-foreground">Pension:</span>
                                                <span className="text-green-600">
                                                    +${state.retirement.pension.monthlyAmount?.toLocaleString()}
                                                </span>
                                            </div>
                                        )}
                                        <Separator />
                                        <div className="flex justify-between font-bold">
                                            <span>Gap to Fill from Savings:</span>
                                            <span className="text-orange-600">
                                                ${retirementIncomeGap.gap.toLocaleString()}/month
                                            </span>
                                        </div>
                                        <div className="text-xs text-muted-foreground">
                                            Using 4% rule, you need ~${(retirementIncomeGap.gap * 12 * 25).toLocaleString()} saved
                                        </div>
                                    </div>
                                </div>
                            </>
                        )}
                    </CardContent>
                </Card>
            )}

            {/* Life Events */}
            {state.lifeEvents.length > 0 && (
                <Card>
                    <CardHeader>
                        <CardTitle>Planned Life Events</CardTitle>
                        <CardDescription>{state.lifeEvents.length} event(s) scheduled</CardDescription>
                    </CardHeader>
                    <CardContent>
                        <div className="space-y-2">
                            {state.lifeEvents.map((event) => (
                                <div key={event.id} className="flex justify-between items-center py-2 border-b last:border-0">
                                    <div>
                                        <div className="font-medium">{event.description}</div>
                                        <div className="text-sm text-muted-foreground">
                                            In {event.yearsFromNow} year{event.yearsFromNow !== 1 ? 's' : ''}
                                            {event.recurring && ` (${event.recurring.duration} years)`}
                                        </div>
                                    </div>
                                    <div className="text-right">
                                        <div className={event.type === "Inheritance" ? "text-green-600" : "text-red-600"}>
                                            {event.type === "Inheritance" ? '+' : '-'}${Math.abs(event.amount).toLocaleString()}
                                        </div>
                                    </div>
                                </div>
                            ))}
                        </div>
                    </CardContent>
                </Card>
            )}

            {/* Assumptions */}
            <Card>
                <CardHeader>
                    <CardTitle>Simulation Assumptions</CardTitle>
                    <CardDescription>Standard assumptions for Monte Carlo simulation</CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
                        <div>
                            <div className="font-medium mb-2">Market Returns</div>
                            <div className="space-y-1 text-muted-foreground">
                                <div>Stock/Bond Portfolio: 8% ± 15%</div>
                                <div>Cash: 3% (fixed)</div>
                                <div>Real Estate: 4% ± 3.5%</div>
                            </div>
                        </div>
                        <div>
                            <div className="font-medium mb-2">Inflation & Taxes</div>
                            <div className="space-y-1 text-muted-foreground">
                                <div>Inflation: 3% ± 2%</div>
                                <div>Filing Status: {state.personalInfo.filingStatus}</div>
                                <div>State: {state.personalInfo.state}</div>
                                <div>Capital Gains: 15%</div>
                            </div>
                        </div>
                    </div>

                    <Alert>
                        <Info className="h-4 w-4" />
                        <AlertDescription>
                            The simulation will run 1,000 Monte Carlo iterations to show you a range of possible outcomes
                            based on historical market volatility.
                        </AlertDescription>
                    </Alert>
                </CardContent>
            </Card>

            {/* Validation Warnings */}
            {monthlyCashFlow.remaining < 0 && (
                <Alert variant="destructive">
                    <AlertTriangle className="h-4 w-4" />
                    <AlertDescription>
                        <strong>Warning:</strong> Your current expenses exceed income.
                        You may want to review your budget before running the simulation.
                    </AlertDescription>
                </Alert>
            )}

            {calculations.liquidSavings < 10000 && (
                <Alert>
                    <AlertTriangle className="h-4 w-4" />
                    <AlertDescription>
                        Your emergency fund appears low. Consider building 3-6 months of expenses in liquid savings.
                    </AlertDescription>
                </Alert>
            )}

            {/* Error Display */}
            {error && (
                <Alert variant="destructive">
                    <AlertTriangle className="h-4 w-4" />
                    <AlertDescription>{error}</AlertDescription>
                </Alert>
            )}

            {/* Action Buttons */}
            <div className="flex gap-4 pt-4">
                <Button
                    size="lg"
                    className="flex-1"
                    onClick={handleRunSimulation}
                    disabled={isSubmitting}
                >
                    {isSubmitting ? (
                        <>
                            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                            Running Simulation...
                        </>
                    ) : (
                        <>
                            <Rocket className="mr-2 h-4 w-4" />
                            Run Simulation
                        </>
                    )}
                </Button>

                <Button
                    size="lg"
                    variant="outline"
                    onClick={handleSaveDraft}
                    disabled={isSubmitting}
                >
                    <Save className="mr-2 h-4 w-4" />
                    Save Draft
                </Button>
            </div>
        </div>
    );
}
