"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Button } from "@/components/ui/button";
import { Calendar } from "@/components/ui/calendar";
import { CalendarIcon } from "lucide-react";
import { format } from "date-fns";
import { cn } from "@/lib/utils";
import { useWizardStore } from "../hooks/useWizardStore";
import { FilingStatus, UsState } from "../types";
import { US_STATES } from "../utils/taxData";

export function AboutYouStep() {
    const personalInfo = useWizardStore((state) => state.personalInfo);
    const setPersonalInfo = useWizardStore((state) => state.setPersonalInfo);

    const calculateAge = (birthDate: Date) => {
        const today = new Date();
        let age = today.getFullYear() - birthDate.getFullYear();
        const monthDiff = today.getMonth() - birthDate.getMonth();
        if (monthDiff < 0 || (monthDiff === 0 && today.getDate() < birthDate.getDate())) {
            age--;
        }
        return age;
    };

    const currentAge = personalInfo.birthDate ? calculateAge(personalInfo.birthDate) : null;

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Tell me about yourself</h2>
                <p className="text-muted-foreground mt-2">
                    This helps us provide accurate tax calculations and age-based recommendations.
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>When were you born?</CardTitle>
                    <CardDescription>
                        We'll use this to calculate your current age and plan for age-based events like retirement
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="space-y-2">
                        <Label>Birth Date</Label>
                        <Popover>
                            <PopoverTrigger asChild>
                                <Button
                                    variant="outline"
                                    className={cn(
                                        "w-full justify-start text-left font-normal",
                                        !personalInfo.birthDate && "text-muted-foreground"
                                    )}
                                >
                                    <CalendarIcon className="mr-2 h-4 w-4" />
                                    {personalInfo.birthDate ? (
                                        format(personalInfo.birthDate, "PPP")
                                    ) : (
                                        <span>Pick a date</span>
                                    )}
                                </Button>
                            </PopoverTrigger>
                            <PopoverContent className="w-auto p-0" align="start">
                                <Calendar
                                    mode="single"
                                    selected={personalInfo.birthDate || undefined}
                                    onSelect={(date) => setPersonalInfo({ birthDate: date || null })}
                                    disabled={(date) =>
                                        date > new Date() || date < new Date("1900-01-01")
                                    }
                                    captionLayout="dropdown"
                                    startMonth={new Date(1940, 0)}
                                    endMonth={new Date()}
                                />
                            </PopoverContent>
                        </Popover>
                    </div>

                    {currentAge && (
                        <div className="rounded-lg bg-muted p-3 text-sm">
                            <p className="font-medium">You're currently {currentAge} years old.</p>
                        </div>
                    )}
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>What's your tax filing status?</CardTitle>
                    <CardDescription>
                        This affects your federal tax brackets and helps us calculate your after-tax income
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <RadioGroup
                        value={personalInfo.filingStatus || ""}
                        onValueChange={(value) => setPersonalInfo({ filingStatus: value as FilingStatus })}
                    >
                        <div className="space-y-3">
                            <div className="flex items-center space-x-3">
                                <RadioGroupItem value="Single" id="single" />
                                <Label htmlFor="single" className="cursor-pointer">Single</Label>
                            </div>
                            <div className="flex items-center space-x-3">
                                <RadioGroupItem value="MarriedFilingJointly" id="married-jointly" />
                                <Label htmlFor="married-jointly" className="cursor-pointer">
                                    Married Filing Jointly
                                </Label>
                            </div>
                            <div className="flex items-center space-x-3">
                                <RadioGroupItem value="MarriedFilingSeparately" id="married-separately" />
                                <Label htmlFor="married-separately" className="cursor-pointer">
                                    Married Filing Separately
                                </Label>
                            </div>
                            <div className="flex items-center space-x-3">
                                <RadioGroupItem value="HeadOfHousehold" id="head-of-household" />
                                <Label htmlFor="head-of-household" className="cursor-pointer">
                                    Head of Household
                                </Label>
                            </div>
                        </div>
                    </RadioGroup>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>What state do you live in?</CardTitle>
                    <CardDescription>
                        Different states have different income tax rates that affect your take-home pay
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="space-y-2">
                        <Label htmlFor="state">State</Label>
                        <Select
                            value={personalInfo.state || ""}
                            onValueChange={(value) => setPersonalInfo({ state: value as UsState })}
                        >
                            <SelectTrigger id="state">
                                <SelectValue placeholder="Select your state" />
                            </SelectTrigger>
                            <SelectContent>
                                {US_STATES.map((state) => (
                                    <SelectItem key={state.code} value={state.code}>
                                        {state.name}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>

                    {personalInfo.state && (
                        <div className="rounded-lg bg-muted p-3 text-sm">
                            <p>
                                {US_STATES.find((s) => s.code === personalInfo.state)?.name} has a{" "}
                                <span className="font-medium">
                                    {US_STATES.find((s) => s.code === personalInfo.state)?.topRate}% top marginal
                                </span>{" "}
                                state income tax rate.
                            </p>
                        </div>
                    )}
                </CardContent>
            </Card>
        </div>
    );
}
