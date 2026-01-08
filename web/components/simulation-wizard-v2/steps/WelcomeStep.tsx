"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { useWizardStore } from "../hooks/useWizardStore";
import { RetirementGoalType } from "../types";

export function WelcomeStep() {
    const simulationName = useWizardStore((state) => state.simulationName);
    const goal = useWizardStore((state) => state.goal);
    const setSimulationName = useWizardStore((state) => state.setSimulationName);
    const setGoal = useWizardStore((state) => state.setGoal);

    const goals: { value: RetirementGoalType; label: string; description: string }[] = [
        {
            value: "CanIRetireEarly",
            label: "Can I retire early?",
            description: "Explore whether you can leave work before traditional retirement age",
        },
        {
            value: "HowMuchToSave",
            label: "How much do I need to save?",
            description: "Figure out your retirement savings target",
        },
        {
            value: "WillMoneyLast",
            label: "Will my money last in retirement?",
            description: "Test if your savings can support your retirement lifestyle",
        },
        {
            value: "JustExploring",
            label: "I'm just exploring",
            description: "Learn about financial planning and see different scenarios",
        },
    ];

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Welcome! Let's plan your financial future.</h2>
                <p className="text-muted-foreground mt-2">
                    In the next few minutes, I'll ask you about your current financial situation, income and expenses,
                    and your retirement goals. At the end, we'll run thousands of simulations to show you different
                    possible futures based on market conditions.
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>Let's give this plan a name</CardTitle>
                    <CardDescription>
                        Choose something meaningful to help you remember this scenario
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <div className="space-y-2">
                        <Label htmlFor="simulation-name">Plan Name</Label>
                        <Input
                            id="simulation-name"
                            placeholder="My Retirement Plan"
                            value={simulationName}
                            onChange={(e) => setSimulationName(e.target.value)}
                            className="text-base"
                        />
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>What are you hoping to learn?</CardTitle>
                    <CardDescription>
                        This helps us provide relevant guidance throughout the process
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <RadioGroup
                        value={goal || ""}
                        onValueChange={(value) => setGoal(value as RetirementGoalType)}
                    >
                        <div className="space-y-3">
                            {goals.map((goalOption) => (
                                <div key={goalOption.value} className="flex items-start space-x-3">
                                    <RadioGroupItem
                                        value={goalOption.value}
                                        id={goalOption.value}
                                        className="mt-1"
                                    />
                                    <Label
                                        htmlFor={goalOption.value}
                                        className="flex-1 cursor-pointer space-y-1"
                                    >
                                        <div className="font-medium">{goalOption.label}</div>
                                        <div className="text-sm text-muted-foreground">
                                            {goalOption.description}
                                        </div>
                                    </Label>
                                </div>
                            ))}
                        </div>
                    </RadioGroup>
                </CardContent>
            </Card>

            <div className="rounded-lg bg-muted p-4 text-sm text-muted-foreground">
                <p className="font-medium mb-1">💡 What to expect:</p>
                <ul className="list-disc list-inside space-y-1">
                    <li>This will take about 10-15 minutes to complete</li>
                    <li>You can skip sections and come back later</li>
                    <li>Your progress is saved automatically</li>
                    <li>All calculations happen on our servers - your data is secure</li>
                </ul>
            </div>
        </div>
    );
}
