"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Target, TrendingUp } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";

const SOCIAL_SECURITY_CLAIMING_AGES = [
    { age: 62, label: "62 (Early - Reduced Benefit)", factor: 0.7 },
    { age: 67, label: "67 (Full Retirement Age)", factor: 1.0 },
    { age: 70, label: "70 (Delayed - Increased Benefit)", factor: 1.24 },
];

export function RetirementGoalsStep() {
    const retirement = useWizardStore((state) => state.retirement);
    const setRetirement = useWizardStore((state) => state.setRetirement);
    const income = useWizardStore((state) => state.income);
    const personalInfo = useWizardStore((state) => state.personalInfo);

    const currentAge = React.useMemo(() => {
        if (!personalInfo.birthDate) return null;
        const today = new Date();
        const birthDate = new Date(personalInfo.birthDate);
        let age = today.getFullYear() - birthDate.getFullYear();
        const monthDiff = today.getMonth() - birthDate.getMonth();
        if (monthDiff < 0 || (monthDiff === 0 && today.getDate() < birthDate.getDate())) {
            age--;
        }
        return age;
    }, [personalInfo.birthDate]);

    const yearsToRetirement = React.useMemo(() => {
        if (!currentAge || !retirement.targetAge) return null;
        return Math.max(0, retirement.targetAge - currentAge);
    }, [currentAge, retirement.targetAge]);

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    const suggestedRetirementAge = currentAge ? Math.max(62, Math.min(70, currentAge + 30)) : 65;

    const fourPercentRule = React.useMemo(() => {
        if (!retirement.targetIncome) return 0;

        // Subtract Social Security if applicable
        let targetFromSavings = retirement.targetIncome;
        if (retirement.socialSecurity.hasSSI && retirement.socialSecurity.estimatedBenefit) {
            targetFromSavings -= retirement.socialSecurity.estimatedBenefit * 12;
        }
        if (retirement.pension.hasPension && retirement.pension.monthlyAmount) {
            targetFromSavings -= retirement.pension.monthlyAmount * 12;
        }

        targetFromSavings = Math.max(0, targetFromSavings);

        // 4% rule: need 25x annual expenses
        return targetFromSavings * 25;
    }, [retirement.targetIncome, retirement.socialSecurity, retirement.pension]);

    const incomeReplacementOptions = React.useMemo(() => {
        if (!income.salary || income.salary === 0) return [];

        return [
            { percentage: 70, amount: income.salary * 0.7, label: "70% of current income (conservative)" },
            { percentage: 80, amount: income.salary * 0.8, label: "80% of current income (moderate)" },
            { percentage: 85, amount: income.salary * 0.85, label: "85% of current income (comfortable)" },
        ];
    }, [income.salary]);

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Let's dream about retirement! 🏖️</h2>
                <p className="text-muted-foreground mt-2">
                    Define your retirement timeline and income needs to see if you're on track.
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>At what age do you want to retire?</CardTitle>
                    <CardDescription>
                        When do you see yourself leaving full-time work?
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="space-y-2">
                        <Label htmlFor="retirement-age">Retirement Age</Label>
                        <Input
                            id="retirement-age"
                            type="number"
                            value={retirement.targetAge || ""}
                            onChange={(e) => setRetirement({ targetAge: parseInt(e.target.value) || null })}
                            placeholder={suggestedRetirementAge.toString()}
                            min={currentAge || 50}
                            max="100"
                        />
                    </div>

                    {retirement.targetAge && currentAge && (
                        <div className="rounded-lg bg-muted p-3 text-sm space-y-1">
                            <p>
                                That's <span className="font-medium">{yearsToRetirement} years</span> from now
                                {retirement.targetAge && `, in ${new Date().getFullYear() + yearsToRetirement!}`}
                            </p>
                            {retirement.targetAge < 62 && (
                                <p className="text-orange-600 text-xs mt-1">
                                    ⚠️ Retiring before 62 means no Social Security eligibility yet
                                </p>
                            )}
                        </div>
                    )}
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>How much annual income do you need in retirement?</CardTitle>
                    <CardDescription>
                        Estimate your retirement expenses in today's dollars
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    {incomeReplacementOptions.length > 0 && (
                        <div className="space-y-3">
                            <Label className="text-sm">Use a rule of thumb:</Label>
                            <RadioGroup
                                value={retirement.targetIncome?.toString() || ""}
                                onValueChange={(value) => setRetirement({ targetIncome: parseFloat(value) || null })}
                            >
                                {incomeReplacementOptions.map((option) => (
                                    <div key={option.percentage} className="flex items-center space-x-2">
                                        <RadioGroupItem value={option.amount.toString()} id={`income-${option.percentage}`} />
                                        <Label htmlFor={`income-${option.percentage}`} className="cursor-pointer text-sm font-normal">
                                            {option.label} ({formatCurrency(option.amount)}/year)
                                        </Label>
                                    </div>
                                ))}
                            </RadioGroup>

                            <div className="relative">
                                <div className="absolute inset-0 flex items-center">
                                    <span className="w-full border-t" />
                                </div>
                                <div className="relative flex justify-center text-xs uppercase">
                                    <span className="bg-background px-2 text-muted-foreground">Or</span>
                                </div>
                            </div>
                        </div>
                    )}

                    <div className="space-y-2">
                        <Label htmlFor="target-income">Specific Annual Amount</Label>
                        <MoneyInput
                            value={retirement.targetIncome || 0}
                            onChange={(value) => setRetirement({ targetIncome: value })}
                            placeholder="85000"
                            showHelper
                            helperFrequency="month"
                        />
                    </div>

                    {retirement.targetIncome && retirement.targetIncome > 0 && (
                        <div className="rounded-lg bg-blue-50 dark:bg-blue-950 p-3 text-sm space-y-1 border border-blue-200 dark:border-blue-800">
                            <p className="text-blue-900 dark:text-blue-100">
                                <span className="font-medium">4% Rule:</span> You'd need about{" "}
                                <span className="font-bold">{formatCurrency(fourPercentRule)}</span> saved
                            </p>
                            <p className="text-xs text-blue-700 dark:text-blue-300">
                                This is a rough estimate. The 4% rule suggests you can withdraw 4% of your savings annually in retirement.
                            </p>
                        </div>
                    )}
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Do you expect to receive Social Security?</CardTitle>
                    <CardDescription>
                        Social Security can be a significant part of retirement income
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="flex items-center space-x-2">
                        <Checkbox
                            id="has-ssi"
                            checked={retirement.socialSecurity.hasSSI}
                            onCheckedChange={(checked) =>
                                setRetirement({
                                    socialSecurity: { ...retirement.socialSecurity, hasSSI: !!checked },
                                })
                            }
                        />
                        <Label htmlFor="has-ssi" className="cursor-pointer">
                            Yes, I expect to receive Social Security benefits
                        </Label>
                    </div>

                    {retirement.socialSecurity.hasSSI && (
                        <div className="pl-6 space-y-4 border-l-2">
                            <div className="space-y-2">
                                <Label htmlFor="ssi-benefit">Estimated Monthly Benefit (at full retirement age)</Label>
                                <MoneyInput
                                    value={retirement.socialSecurity.estimatedBenefit || 0}
                                    onChange={(value) =>
                                        setRetirement({
                                            socialSecurity: { ...retirement.socialSecurity, estimatedBenefit: value },
                                        })
                                    }
                                    placeholder="2400"
                                />
                                <p className="text-xs text-muted-foreground">
                                    Check your estimated benefit at{" "}
                                    <a
                                        href="https://www.ssa.gov/myaccount/"
                                        target="_blank"
                                        rel="noopener noreferrer"
                                        className="text-blue-600 hover:underline"
                                    >
                                        ssa.gov/myaccount
                                    </a>
                                </p>
                            </div>

                            {retirement.socialSecurity.estimatedBenefit && retirement.socialSecurity.estimatedBenefit > 0 && (
                                <div className="space-y-3">
                                    <Label>When do you plan to claim?</Label>
                                    <RadioGroup
                                        value={retirement.socialSecurity.claimingAge?.toString() || "67"}
                                        onValueChange={(value) =>
                                            setRetirement({
                                                socialSecurity: {
                                                    ...retirement.socialSecurity,
                                                    claimingAge: parseInt(value),
                                                },
                                            })
                                        }
                                    >
                                        {SOCIAL_SECURITY_CLAIMING_AGES.map((option) => {
                                            const adjustedBenefit = (retirement.socialSecurity.estimatedBenefit || 0) * option.factor;
                                            return (
                                                <div key={option.age} className="flex items-start space-x-2">
                                                    <RadioGroupItem value={option.age.toString()} id={`claim-${option.age}`} className="mt-1" />
                                                    <Label htmlFor={`claim-${option.age}`} className="cursor-pointer flex-1">
                                                        <div className="space-y-0.5">
                                                            <div className="font-medium">{option.label}</div>
                                                            <div className="text-xs text-muted-foreground">
                                                                ~{formatCurrency(adjustedBenefit)}/month ({formatCurrency(adjustedBenefit * 12)}/year)
                                                            </div>
                                                        </div>
                                                    </Label>
                                                </div>
                                            );
                                        })}
                                    </RadioGroup>

                                    <div className="rounded-lg bg-muted p-2 text-xs text-muted-foreground">
                                        💡 Delaying until 70 increases your benefit, but claiming early at 62 may make sense if you need the income or have health concerns.
                                    </div>
                                </div>
                            )}
                        </div>
                    )}
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Do you expect any pension income?</CardTitle>
                    <CardDescription>
                        Traditional employer pensions or annuities
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="flex items-center space-x-2">
                        <Checkbox
                            id="has-pension"
                            checked={retirement.pension.hasPension}
                            onCheckedChange={(checked) =>
                                setRetirement({
                                    pension: { ...retirement.pension, hasPension: !!checked },
                                })
                            }
                        />
                        <Label htmlFor="has-pension" className="cursor-pointer">
                            Yes, I expect pension income
                        </Label>
                    </div>

                    {retirement.pension.hasPension && (
                        <div className="pl-6 space-y-4 border-l-2">
                            <div className="space-y-2">
                                <Label htmlFor="pension-amount">Monthly Pension Amount</Label>
                                <MoneyInput
                                    value={retirement.pension.monthlyAmount || 0}
                                    onChange={(value) =>
                                        setRetirement({
                                            pension: { ...retirement.pension, monthlyAmount: value },
                                        })
                                    }
                                    placeholder="1500"
                                />
                            </div>

                            <div className="space-y-2">
                                <Label htmlFor="pension-age">Starting at Age</Label>
                                <Input
                                    id="pension-age"
                                    type="number"
                                    value={retirement.pension.startAge || ""}
                                    onChange={(e) =>
                                        setRetirement({
                                            pension: { ...retirement.pension, startAge: parseInt(e.target.value) || undefined },
                                        })
                                    }
                                    placeholder="65"
                                    min="50"
                                    max="100"
                                />
                            </div>
                        </div>
                    )}
                </CardContent>
            </Card>

            {retirement.targetIncome && retirement.targetIncome > 0 && (
                <Card>
                    <CardHeader>
                        <CardTitle className="flex items-center gap-2">
                            <Target className="h-5 w-5" />
                            Retirement Income Plan
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="space-y-3 text-sm">
                            <div className="flex justify-between items-center py-2 border-b">
                                <span className="text-muted-foreground">Target Annual Income:</span>
                                <span className="font-medium">{formatCurrency(retirement.targetIncome)}</span>
                            </div>

                            {retirement.socialSecurity.hasSSI && retirement.socialSecurity.estimatedBenefit && (
                                <div className="flex justify-between items-center py-2 border-b">
                                    <span className="text-muted-foreground">
                                        Social Security (at {retirement.socialSecurity.claimingAge || 67}):
                                    </span>
                                    <span className="text-green-600 font-medium">
                                        +{formatCurrency((retirement.socialSecurity.estimatedBenefit || 0) *
                                            (SOCIAL_SECURITY_CLAIMING_AGES.find(a => a.age === (retirement.socialSecurity.claimingAge || 67))?.factor || 1) * 12)}
                                    </span>
                                </div>
                            )}

                            {retirement.pension.hasPension && retirement.pension.monthlyAmount && (
                                <div className="flex justify-between items-center py-2 border-b">
                                    <span className="text-muted-foreground">Pension:</span>
                                    <span className="text-green-600 font-medium">
                                        +{formatCurrency((retirement.pension.monthlyAmount || 0) * 12)}
                                    </span>
                                </div>
                            )}

                            <div className="flex justify-between items-center py-2 font-medium">
                                <span>Gap to fill from savings:</span>
                                <span className="text-lg">
                                    {formatCurrency(Math.max(0, retirement.targetIncome -
                                        ((retirement.socialSecurity.hasSSI && retirement.socialSecurity.estimatedBenefit ?
                                            (retirement.socialSecurity.estimatedBenefit *
                                                (SOCIAL_SECURITY_CLAIMING_AGES.find(a => a.age === (retirement.socialSecurity.claimingAge || 67))?.factor || 1) * 12) : 0) +
                                            (retirement.pension.hasPension && retirement.pension.monthlyAmount ?
                                                retirement.pension.monthlyAmount * 12 : 0))))}
                                </span>
                            </div>

                            <div className="rounded-lg bg-muted p-3 space-y-1">
                                <p className="text-xs font-medium">Based on the 4% rule:</p>
                                <p className="text-xs text-muted-foreground">
                                    You need approximately <span className="font-bold">{formatCurrency(fourPercentRule)}</span> saved
                                </p>
                            </div>
                        </div>
                    </CardContent>
                </Card>
            )}
        </div>
    );
}
