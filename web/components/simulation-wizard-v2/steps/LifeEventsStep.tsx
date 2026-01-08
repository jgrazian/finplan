"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Plus, Trash2, Calendar, DollarSign } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";
import { LifeEvent, LifeEventType } from "../types";

const LIFE_EVENT_TYPES: {
    value: LifeEventType;
    label: string;
    description: string;
    defaultAmount: number;
    icon: string;
}[] = [
        {
            value: "Wedding",
            label: "Wedding",
            description: "Marriage ceremony and celebration",
            defaultAmount: 30000,
            icon: "💒"
        },
        {
            value: "ChildEducation",
            label: "Child's Education",
            description: "College or university expenses",
            defaultAmount: 40000,
            icon: "🎓"
        },
        {
            value: "MajorPurchase",
            label: "Major Purchase",
            description: "Car, boat, or other large expense",
            defaultAmount: 35000,
            icon: "🚗"
        },
        {
            value: "HomeRenovation",
            label: "Home Renovation",
            description: "Remodel, addition, or major repairs",
            defaultAmount: 50000,
            icon: "🏗️"
        },
        {
            value: "CareerChange",
            label: "Career Change",
            description: "Job transition or sabbatical",
            defaultAmount: 10000,
            icon: "💼"
        },
        {
            value: "Inheritance",
            label: "Expected Inheritance",
            description: "Anticipated windfall from family",
            defaultAmount: 100000,
            icon: "🎁"
        },
        {
            value: "StartBusiness",
            label: "Start a Business",
            description: "Initial investment for new venture",
            defaultAmount: 50000,
            icon: "🚀"
        },
        {
            value: "Healthcare",
            label: "Healthcare Event",
            description: "Planned surgery or medical expense",
            defaultAmount: 15000,
            icon: "🏥"
        },
        {
            value: "Custom",
            label: "Other Life Event",
            description: "Any other major financial event",
            defaultAmount: 0,
            icon: "📅"
        },
    ];

export function LifeEventsStep() {
    const lifeEvents = useWizardStore((state) => state.lifeEvents);
    const addLifeEvent = useWizardStore((state) => state.addLifeEvent);
    const updateLifeEvent = useWizardStore((state) => state.updateLifeEvent);
    const removeLifeEvent = useWizardStore((state) => state.removeLifeEvent);
    const currentAge = useWizardStore((state) => {
        if (!state.personalInfo.birthDate) return null;
        const today = new Date();
        const birthDate = new Date(state.personalInfo.birthDate);
        let age = today.getFullYear() - birthDate.getFullYear();
        const monthDiff = today.getMonth() - birthDate.getMonth();
        if (monthDiff < 0 || (monthDiff === 0 && today.getDate() < birthDate.getDate())) {
            age--;
        }
        return age;
    });

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    const handleAddEvent = (type: LifeEventType) => {
        const eventType = LIFE_EVENT_TYPES.find((t) => t.value === type);
        const id = `event-${Date.now()}`;

        addLifeEvent({
            id,
            type,
            description: eventType?.label || "Life Event",
            yearsFromNow: 5,
            amount: eventType?.defaultAmount || 0,
        });
    };

    const calculateEventYear = (yearsFromNow: number) => {
        return new Date().getFullYear() + yearsFromNow;
    };

    const sortedEvents = React.useMemo(() => {
        return [...lifeEvents].sort((a, b) => a.yearsFromNow - b.yearsFromNow);
    }, [lifeEvents]);

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Plan for life's big moments</h2>
                <p className="text-muted-foreground mt-2">
                    Life doesn't always go according to plan. Let's account for major events and expenses.
                </p>
            </div>

            {lifeEvents.length === 0 ? (
                <Card>
                    <CardHeader>
                        <CardTitle>Are you expecting any major life events?</CardTitle>
                        <CardDescription>
                            Select the events you anticipate in your future
                        </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3">
                        <div className="grid gap-2">
                            {LIFE_EVENT_TYPES.map((eventType) => (
                                <Button
                                    key={eventType.value}
                                    variant="outline"
                                    className="justify-start h-auto py-3"
                                    onClick={() => handleAddEvent(eventType.value)}
                                >
                                    <span className="text-2xl mr-3">{eventType.icon}</span>
                                    <div className="flex flex-col items-start flex-1">
                                        <span className="font-medium">{eventType.label}</span>
                                        <span className="text-xs text-muted-foreground">{eventType.description}</span>
                                    </div>
                                    <Plus className="h-4 w-4 ml-2" />
                                </Button>
                            ))}
                        </div>

                        <Button variant="ghost" className="w-full" onClick={() => { }}>
                            Skip - No major events planned
                        </Button>
                    </CardContent>
                </Card>
            ) : (
                <>
                    {sortedEvents.map((event) => {
                        const eventTypeInfo = LIFE_EVENT_TYPES.find((t) => t.value === event.type);
                        const isIncome = event.type === "Inheritance";

                        return (
                            <Card key={event.id}>
                                <CardHeader>
                                    <div className="flex items-start justify-between">
                                        <div className="flex items-start gap-3">
                                            <span className="text-2xl">{eventTypeInfo?.icon || "📅"}</span>
                                            <div>
                                                <CardTitle>{eventTypeInfo?.label || event.type}</CardTitle>
                                                <CardDescription>{eventTypeInfo?.description}</CardDescription>
                                            </div>
                                        </div>
                                        <Button
                                            variant="ghost"
                                            size="icon"
                                            onClick={() => removeLifeEvent(event.id)}
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                </CardHeader>
                                <CardContent className="space-y-4">
                                    {event.type === "Custom" && (
                                        <div className="space-y-2">
                                            <Label htmlFor={`description-${event.id}`}>Event Description</Label>
                                            <Input
                                                id={`description-${event.id}`}
                                                value={event.description}
                                                onChange={(e) => updateLifeEvent(event.id, { description: e.target.value })}
                                                placeholder="e.g., Family vacation, Home purchase"
                                            />
                                        </div>
                                    )}

                                    <div className="grid grid-cols-2 gap-4">
                                        <div className="space-y-2">
                                            <Label htmlFor={`years-${event.id}`} className="flex items-center gap-2">
                                                <Calendar className="h-4 w-4" />
                                                Years from Now
                                            </Label>
                                            <Input
                                                id={`years-${event.id}`}
                                                type="number"
                                                value={event.yearsFromNow}
                                                onChange={(e) => updateLifeEvent(event.id, { yearsFromNow: parseInt(e.target.value) || 0 })}
                                                min="0"
                                                max="50"
                                            />
                                            <p className="text-xs text-muted-foreground">
                                                In {calculateEventYear(event.yearsFromNow)}
                                                {currentAge && ` (age ${currentAge + event.yearsFromNow})`}
                                            </p>
                                        </div>

                                        <div className="space-y-2">
                                            <Label htmlFor={`amount-${event.id}`} className="flex items-center gap-2">
                                                <DollarSign className="h-4 w-4" />
                                                {isIncome ? "Expected Amount" : "Estimated Cost"}
                                            </Label>
                                            <MoneyInput
                                                value={event.amount}
                                                onChange={(value) => updateLifeEvent(event.id, { amount: value })}
                                                placeholder="0"
                                            />
                                        </div>
                                    </div>

                                    {(event.type === "ChildEducation" || event.type === "CareerChange") && (
                                        <div className="space-y-3">
                                            <div className="flex items-center space-x-2">
                                                <Checkbox
                                                    id={`recurring-${event.id}`}
                                                    checked={!!event.recurring}
                                                    onCheckedChange={(checked) => {
                                                        if (checked) {
                                                            updateLifeEvent(event.id, {
                                                                recurring: { duration: 4, inflationAdjusted: true },
                                                            });
                                                        } else {
                                                            updateLifeEvent(event.id, { recurring: undefined });
                                                        }
                                                    }}
                                                />
                                                <Label htmlFor={`recurring-${event.id}`} className="cursor-pointer">
                                                    This is a recurring expense
                                                </Label>
                                            </div>

                                            {event.recurring && (
                                                <div className="pl-6 space-y-3 border-l-2">
                                                    <div className="space-y-2">
                                                        <Label htmlFor={`duration-${event.id}`}>Duration (years)</Label>
                                                        <Input
                                                            id={`duration-${event.id}`}
                                                            type="number"
                                                            value={event.recurring.duration}
                                                            onChange={(e) =>
                                                                updateLifeEvent(event.id, {
                                                                    recurring: {
                                                                        ...event.recurring!,
                                                                        duration: parseInt(e.target.value) || 1
                                                                    },
                                                                })
                                                            }
                                                            min="1"
                                                            max="20"
                                                        />
                                                        <p className="text-xs text-muted-foreground">
                                                            Total cost: {formatCurrency(event.amount * event.recurring.duration)}
                                                        </p>
                                                    </div>

                                                    <div className="flex items-center space-x-2">
                                                        <Checkbox
                                                            id={`inflation-${event.id}`}
                                                            checked={event.recurring.inflationAdjusted}
                                                            onCheckedChange={(checked) =>
                                                                updateLifeEvent(event.id, {
                                                                    recurring: { ...event.recurring!, inflationAdjusted: !!checked },
                                                                })
                                                            }
                                                        />
                                                        <Label htmlFor={`inflation-${event.id}`} className="cursor-pointer text-sm">
                                                            Adjust for inflation each year
                                                        </Label>
                                                    </div>
                                                </div>
                                            )}
                                        </div>
                                    )}

                                    {event.amount > 0 && (
                                        <div className="rounded-lg bg-muted p-3 text-sm">
                                            <p className="font-medium">
                                                {isIncome ? "💰 Expected Income" : "💸 Expected Expense"}
                                            </p>
                                            <p className="text-muted-foreground mt-1">
                                                {formatCurrency(event.amount)} in {event.yearsFromNow} year{event.yearsFromNow !== 1 ? "s" : ""}{" "}
                                                ({calculateEventYear(event.yearsFromNow)})
                                                {event.recurring && ` for ${event.recurring.duration} years`}
                                            </p>
                                        </div>
                                    )}
                                </CardContent>
                            </Card>
                        );
                    })}

                    <Card>
                        <CardHeader>
                            <CardTitle>Add Another Event</CardTitle>
                        </CardHeader>
                        <CardContent>
                            <div className="grid gap-2">
                                {LIFE_EVENT_TYPES.map((eventType) => (
                                    <Button
                                        key={`add-${eventType.value}`}
                                        variant="outline"
                                        className="justify-start h-auto py-2"
                                        onClick={() => handleAddEvent(eventType.value)}
                                    >
                                        <span className="text-xl mr-2">{eventType.icon}</span>
                                        <div className="flex flex-col items-start flex-1">
                                            <span className="font-medium text-sm">{eventType.label}</span>
                                            <span className="text-xs text-muted-foreground">{eventType.description}</span>
                                        </div>
                                        <Plus className="h-4 w-4 ml-2" />
                                    </Button>
                                ))}
                            </div>
                        </CardContent>
                    </Card>

                    {lifeEvents.length > 0 && (
                        <Card>
                            <CardHeader>
                                <CardTitle className="flex items-center gap-2">
                                    <Calendar className="h-5 w-5" />
                                    Life Events Timeline
                                </CardTitle>
                            </CardHeader>
                            <CardContent>
                                <div className="space-y-2">
                                    {sortedEvents.map((event) => {
                                        const eventTypeInfo = LIFE_EVENT_TYPES.find((t) => t.value === event.type);
                                        const isIncome = event.type === "Inheritance";

                                        return (
                                            <div
                                                key={event.id}
                                                className="flex items-center justify-between py-2 border-b last:border-0"
                                            >
                                                <div className="flex items-center gap-2">
                                                    <span className="text-lg">{eventTypeInfo?.icon || "📅"}</span>
                                                    <div>
                                                        <p className="text-sm font-medium">{event.description}</p>
                                                        <p className="text-xs text-muted-foreground">
                                                            {calculateEventYear(event.yearsFromNow)}
                                                            {currentAge && ` • Age ${currentAge + event.yearsFromNow}`}
                                                            {event.recurring && ` • ${event.recurring.duration} years`}
                                                        </p>
                                                    </div>
                                                </div>
                                                <div className={`text-sm font-medium ${isIncome ? "text-green-600" : ""}`}>
                                                    {isIncome ? "+" : "-"}{formatCurrency(event.amount)}
                                                </div>
                                            </div>
                                        );
                                    })}
                                </div>
                            </CardContent>
                        </Card>
                    )}
                </>
            )}
        </div>
    );
}
