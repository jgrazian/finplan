"use client";

import * as React from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardFooter } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Calendar } from "@/components/ui/calendar";
import { cn } from "@/lib/utils";
import { format } from "date-fns";
import { CalendarIcon } from "lucide-react";
import { StepProps } from "../types";
import { MoneyInput } from "../MoneyInput";
import { PortfolioListItem, SavedPortfolio } from "@/lib/types";

export function BasicsStep({
    name,
    setName,
    description,
    setDescription,
    parameters,
    updateParameters,
    portfolios,
    selectedPortfolioId,
    setSelectedPortfolioId,
    selectedPortfolio,
    loadingPortfolio,
}: StepProps & {
    name: string;
    setName: (v: string) => void;
    description: string;
    setDescription: (v: string) => void;
    portfolios: PortfolioListItem[];
    selectedPortfolioId?: string;
    setSelectedPortfolioId: (id: string | undefined) => void;
    selectedPortfolio: SavedPortfolio | null;
    loadingPortfolio: boolean;
}) {
    const [startDate, setStartDate] = React.useState<Date | undefined>(
        parameters.start_date ? new Date(parameters.start_date) : undefined
    );
    const [birthDate, setBirthDate] = React.useState<Date | undefined>(
        parameters.birth_date ? new Date(parameters.birth_date) : undefined
    );

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(amount);

    return (
        <div className="space-y-6">
            {/* Portfolio Selection */}
            <div className="space-y-4 p-4 bg-muted/50 rounded-lg">
                <div className="flex justify-between items-center">
                    <div>
                        <h3 className="font-medium">Portfolio</h3>
                        <p className="text-sm text-muted-foreground">Select a portfolio to run this simulation against</p>
                    </div>
                    <a href="/portfolios/new" className="text-sm text-primary hover:underline">
                        + Create New Portfolio
                    </a>
                </div>
                <Select
                    value={selectedPortfolioId || "none"}
                    onValueChange={(v) => setSelectedPortfolioId(v === "none" ? undefined : v)}
                >
                    <SelectTrigger>
                        <SelectValue placeholder="Select a portfolio..." />
                    </SelectTrigger>
                    <SelectContent>
                        <SelectItem value="none">No portfolio (define accounts manually)</SelectItem>
                        {portfolios.map((p) => (
                            <SelectItem key={p.id} value={p.id}>
                                {p.name} ({formatCurrency(p.total_value)})
                            </SelectItem>
                        ))}
                    </SelectContent>
                </Select>
                {loadingPortfolio && (
                    <p className="text-sm text-muted-foreground">Loading portfolio...</p>
                )}
                {selectedPortfolio && !loadingPortfolio && (
                    <div className="text-sm bg-background rounded p-3 border">
                        <div className="flex justify-between">
                            <span className="text-muted-foreground">Net Worth:</span>
                            <span className="font-medium">
                                {formatCurrency(selectedPortfolio.accounts.reduce(
                                    (sum, acc) => sum + acc.assets.reduce((s, a) => s + a.initial_value, 0),
                                    0
                                ))}
                            </span>
                        </div>
                        <div className="flex justify-between">
                            <span className="text-muted-foreground">Accounts:</span>
                            <span>{selectedPortfolio.accounts.length}</span>
                        </div>
                        <div className="flex justify-between">
                            <span className="text-muted-foreground">Assets:</span>
                            <span>{selectedPortfolio.accounts.reduce((sum, acc) => sum + acc.assets.length, 0)}</span>
                        </div>
                    </div>
                )}
            </div>

            <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                    <Label htmlFor="name">Simulation Name *</Label>
                    <Input
                        id="name"
                        placeholder="My Retirement Plan"
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                    />
                </div>
                <div className="space-y-2">
                    <Label htmlFor="duration">Duration (years)</Label>
                    <Input
                        id="duration"
                        type="number"
                        min={1}
                        max={100}
                        value={parameters.duration_years}
                        onChange={(e) => updateParameters("duration_years", parseInt(e.target.value) || 30)}
                    />
                </div>
            </div>

            <div className="space-y-2">
                <Label htmlFor="description">Description</Label>
                <Textarea
                    id="description"
                    placeholder="Describe your financial scenario..."
                    value={description}
                    onChange={(e) => setDescription(e.target.value)}
                    rows={3}
                />
            </div>

            <div className="grid gap-4 md:grid-cols-2">
                <div className="space-y-2">
                    <Label>Start Date</Label>
                    <Popover>
                        <PopoverTrigger asChild>
                            <Button
                                variant="outline"
                                className={cn(
                                    "w-full justify-start text-left font-normal",
                                    !startDate && "text-muted-foreground"
                                )}
                            >
                                <CalendarIcon className="mr-2 h-4 w-4" />
                                {startDate ? format(startDate, "PPP") : "Today (default)"}
                            </Button>
                        </PopoverTrigger>
                        <PopoverContent className="w-auto p-0">
                            <Card>
                                <CardContent>
                                    <Calendar
                                        mode="single"
                                        selected={startDate}
                                        onSelect={(date) => {
                                            setStartDate(date);
                                            updateParameters("start_date", date ? format(date, "yyyy-MM-dd") : undefined);
                                        }}
                                        captionLayout="dropdown"
                                    />
                                </CardContent>
                                <CardFooter>
                                    <Button
                                        variant="ghost"
                                        onClick={() => {
                                            setStartDate(undefined);
                                            updateParameters("start_date", undefined);
                                        }}
                                    >
                                        Clear
                                    </Button>
                                </CardFooter>
                            </Card>
                        </PopoverContent>
                    </Popover>
                </div>

                <div className="space-y-2">
                    <Label>Birth Date (for age-based events)</Label>
                    <Popover>
                        <PopoverTrigger asChild>
                            <Button
                                variant="outline"
                                className={cn(
                                    "w-full justify-start text-left font-normal",
                                    !birthDate && "text-muted-foreground"
                                )}
                            >
                                <CalendarIcon className="mr-2 h-4 w-4" />
                                {birthDate ? format(birthDate, "PPP") : "Select birth date"}
                            </Button>
                        </PopoverTrigger>
                        <PopoverContent className="w-auto p-0">
                            <Card>
                                <CardContent>
                                    <Calendar
                                        mode="single"
                                        selected={birthDate}
                                        onSelect={(date) => {
                                            setBirthDate(date);
                                            updateParameters("birth_date", date ? format(date, "yyyy-MM-dd") : undefined);
                                        }}
                                        captionLayout="dropdown"
                                    />
                                </CardContent>
                            </Card>
                        </PopoverContent>
                    </Popover>
                </div>
            </div>

            {/* Retirement Age */}
            <div className="space-y-2">
                <Label htmlFor="retirement-age">Retirement Age</Label>
                <Input
                    id="retirement-age"
                    type="number"
                    min={30}
                    max={100}
                    value={parameters.retirement_age || 65}
                    onChange={(e) => updateParameters("retirement_age", parseInt(e.target.value) || 65)}
                    placeholder="65"
                />
                <p className="text-xs text-muted-foreground">
                    Age when spending targets typically activate and income stops
                </p>
            </div>
        </div>
    );
}
