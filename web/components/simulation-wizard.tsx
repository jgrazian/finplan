"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Wizard } from "@/components/ui/wizard";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Calendar } from "@/components/ui/calendar";
import { cn } from "@/lib/utils";
import { format } from "date-fns";
import { CalendarIcon, Plus, Trash2, ArrowRight, ArrowLeft, Save } from "lucide-react";

// Format number with commas for display
const formatMoney = (value: number): string => {
    return new Intl.NumberFormat("en-US").format(value);
};

// Parse formatted string back to number
const parseMoney = (value: string): number => {
    return parseFloat(value.replace(/,/g, "")) || 0;
};

// Custom hook for money input formatting
function useMoneyInput(initialValue: number, onChange: (value: number) => void) {
    const [displayValue, setDisplayValue] = React.useState(formatMoney(initialValue));
    const [isFocused, setIsFocused] = React.useState(false);

    React.useEffect(() => {
        if (!isFocused) {
            setDisplayValue(formatMoney(initialValue));
        }
    }, [initialValue, isFocused]);

    const handleFocus = (e: React.FocusEvent<HTMLInputElement>) => {
        setIsFocused(true);
        // Show raw number on focus
        setDisplayValue(initialValue.toString());
        // Select all text for easy replacement
        e.target.select();
    };

    const handleBlur = () => {
        setIsFocused(false);
        const numValue = parseMoney(displayValue);
        onChange(numValue);
        setDisplayValue(formatMoney(numValue));
    };

    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        setDisplayValue(e.target.value);
    };

    return {
        value: displayValue,
        onChange: handleChange,
        onFocus: handleFocus,
        onBlur: handleBlur,
    };
}

// MoneyInput component for formatted currency inputs
function MoneyInput({
    value,
    onChange,
    ...props
}: {
    value: number;
    onChange: (value: number) => void;
} & Omit<React.ComponentProps<typeof Input>, 'value' | 'onChange' | 'type'>) {
    const moneyProps = useMoneyInput(value, onChange);
    return <Input {...props} {...moneyProps} />;
}

import {
    SimulationParameters,
    Account,
    Asset,
    CashFlow,
    Event,
    SpendingTarget,
    AccountType,
    AssetClass,
    RepeatInterval,
    InflationProfile,
    ReturnProfile,
    NamedReturnProfile,
    NamedInflationProfile,
    AssetInflationMapping,
    WithdrawalStrategy,
    PortfolioListItem,
    SavedPortfolio,
    DEFAULT_SIMULATION_PARAMETERS,
    DEFAULT_TAX_CONFIG,
    DEFAULT_NAMED_RETURN_PROFILES,
    DEFAULT_NAMED_INFLATION_PROFILES,
} from "@/lib/types";
import { createSimulation, updateSimulation, listPortfolios, getPortfolio } from "@/lib/api";

const WIZARD_STEPS = [
    { id: "basics", title: "Basics", description: "Name, dates & portfolio" },
    { id: "profiles", title: "Profiles", description: "Inflation & returns" },
    { id: "asset-linking", title: "Asset Linking", description: "Link assets to profiles" },
    { id: "cashflows", title: "Cash Flows", description: "Income & expenses" },
    { id: "events", title: "Events", description: "Life events" },
    { id: "spending", title: "Spending", description: "Retirement spending" },
    { id: "review", title: "Review", description: "Final review" },
];

interface SimulationWizardProps {
    initialData?: {
        id?: string;
        name?: string;
        description?: string;
        parameters?: SimulationParameters;
        portfolio_id?: string;
    };
    initialPortfolioId?: string;
    onComplete?: (simulation: { id: string }) => void;
}

export function SimulationWizard({ initialData, initialPortfolioId, onComplete }: SimulationWizardProps) {
    const router = useRouter();
    const [currentStep, setCurrentStep] = React.useState(0);
    const [isSubmitting, setIsSubmitting] = React.useState(false);

    // Portfolio state
    const [portfolios, setPortfolios] = React.useState<PortfolioListItem[]>([]);
    const [selectedPortfolioId, setSelectedPortfolioId] = React.useState<string | undefined>(
        initialPortfolioId || initialData?.portfolio_id
    );
    const [selectedPortfolio, setSelectedPortfolio] = React.useState<SavedPortfolio | null>(null);
    const [loadingPortfolio, setLoadingPortfolio] = React.useState(false);

    // Form state
    const [name, setName] = React.useState(initialData?.name || "");
    const [description, setDescription] = React.useState(initialData?.description || "");
    const [parameters, setParameters] = React.useState<SimulationParameters>(
        initialData?.parameters || { ...DEFAULT_SIMULATION_PARAMETERS }
    );

    // Load portfolios on mount
    React.useEffect(() => {
        listPortfolios().then(setPortfolios).catch(console.error);
    }, []);

    // Load selected portfolio details
    React.useEffect(() => {
        if (selectedPortfolioId) {
            setLoadingPortfolio(true);
            getPortfolio(selectedPortfolioId)
                .then((portfolio) => {
                    setSelectedPortfolio(portfolio);
                    // Update parameters with portfolio accounts
                    setParameters((prev) => ({
                        ...prev,
                        accounts: portfolio.accounts,
                    }));
                })
                .catch(console.error)
                .finally(() => setLoadingPortfolio(false));
        } else {
            setSelectedPortfolio(null);
        }
    }, [selectedPortfolioId]);

    const updateParameters = <K extends keyof SimulationParameters>(
        key: K,
        value: SimulationParameters[K]
    ) => {
        setParameters((prev) => ({ ...prev, [key]: value }));
    };

    const handleNext = () => {
        if (currentStep < WIZARD_STEPS.length - 1) {
            setCurrentStep((prev) => prev + 1);
        }
    };

    const handlePrevious = () => {
        if (currentStep > 0) {
            setCurrentStep((prev) => prev - 1);
        }
    };

    const handleSave = async () => {
        setIsSubmitting(true);
        try {
            let result;
            if (initialData?.id) {
                result = await updateSimulation(initialData.id, {
                    name,
                    description,
                    parameters,
                    portfolio_id: selectedPortfolioId,
                });
            } else {
                result = await createSimulation({
                    name,
                    description,
                    parameters,
                    portfolio_id: selectedPortfolioId,
                });
            }
            onComplete?.(result);
            router.push(`/simulations/${result.id}`);
        } catch (error) {
            console.error("Failed to save simulation:", error);
        } finally {
            setIsSubmitting(false);
        }
    };

    return (
        <div className="space-y-8">
            <Wizard
                steps={WIZARD_STEPS}
                currentStep={currentStep}
                onStepClick={setCurrentStep}
                allowNavigation
            />

            <Card>
                <CardHeader>
                    <CardTitle>{WIZARD_STEPS[currentStep].title}</CardTitle>
                    <CardDescription>{WIZARD_STEPS[currentStep].description}</CardDescription>
                </CardHeader>
                <CardContent>
                    {currentStep === 0 && (
                        <BasicsStep
                            name={name}
                            setName={setName}
                            description={description}
                            setDescription={setDescription}
                            parameters={parameters}
                            updateParameters={updateParameters}
                            portfolios={portfolios}
                            selectedPortfolioId={selectedPortfolioId}
                            setSelectedPortfolioId={setSelectedPortfolioId}
                            selectedPortfolio={selectedPortfolio}
                            loadingPortfolio={loadingPortfolio}
                        />
                    )}
                    {currentStep === 1 && (
                        <ProfilesStep
                            parameters={parameters}
                            updateParameters={updateParameters}
                        />
                    )}
                    {currentStep === 2 && (
                        <AssetLinkingStep
                            parameters={parameters}
                            updateParameters={updateParameters}
                            selectedPortfolio={selectedPortfolio}
                        />
                    )}
                    {currentStep === 3 && (
                        <CashFlowsStep
                            parameters={parameters}
                            updateParameters={updateParameters}
                            selectedPortfolio={selectedPortfolio}
                        />
                    )}
                    {currentStep === 4 && (
                        <EventsStep
                            parameters={parameters}
                            updateParameters={updateParameters}
                        />
                    )}
                    {currentStep === 5 && (
                        <SpendingStep
                            parameters={parameters}
                            updateParameters={updateParameters}
                        />
                    )}
                    {currentStep === 6 && (
                        <ReviewStep
                            name={name}
                            description={description}
                            parameters={parameters}
                            selectedPortfolio={selectedPortfolio}
                        />
                    )}
                </CardContent>
            </Card>

            <div className="flex justify-between">
                <Button
                    variant="outline"
                    onClick={handlePrevious}
                    disabled={currentStep === 0}
                >
                    <ArrowLeft className="mr-2 h-4 w-4" />
                    Previous
                </Button>
                <div className="flex gap-2">
                    {currentStep === WIZARD_STEPS.length - 1 ? (
                        <Button onClick={handleSave} disabled={isSubmitting || !name}>
                            <Save className="mr-2 h-4 w-4" />
                            {isSubmitting ? "Saving..." : "Save Simulation"}
                        </Button>
                    ) : (
                        <Button onClick={handleNext}>
                            Next
                            <ArrowRight className="ml-2 h-4 w-4" />
                        </Button>
                    )}
                </div>
            </div>
        </div>
    );
}

// ============================================================================
// Step Components
// ============================================================================

interface StepProps {
    parameters: SimulationParameters;
    updateParameters: <K extends keyof SimulationParameters>(key: K, value: SimulationParameters[K]) => void;
}

function BasicsStep({
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

function ProfilesStep({ parameters, updateParameters }: StepProps) {
    const [inflationType, setInflationType] = React.useState<string>(() => {
        if (parameters.inflation_profile === "None") return "none";
        if (typeof parameters.inflation_profile === "object") {
            if ("Fixed" in parameters.inflation_profile) return "fixed";
            if ("Normal" in parameters.inflation_profile) return "normal";
        }
        return "normal";
    });

    const [namedProfiles, setNamedProfiles] = React.useState<NamedReturnProfile[]>(
        parameters.named_return_profiles || DEFAULT_NAMED_RETURN_PROFILES
    );

    const [namedInflationProfiles, setNamedInflationProfiles] = React.useState<NamedInflationProfile[]>(
        parameters.named_inflation_profiles || DEFAULT_NAMED_INFLATION_PROFILES
    );

    const getInflationValue = (key: "mean" | "std_dev" | "fixed"): number => {
        const profile = parameters.inflation_profile;
        if (key === "fixed" && typeof profile === "object" && "Fixed" in profile) {
            return Math.round(profile.Fixed * 100 * 10) / 10;
        }
        if (typeof profile === "object" && "Normal" in profile) {
            const value = key === "mean" ? profile.Normal.mean * 100 : profile.Normal.std_dev * 100;
            return Math.round(value * 10) / 10;
        }
        return key === "mean" ? 3.5 : 2.8;
    };

    const updateInflation = (type: string, mean?: number, stdDev?: number, fixed?: number) => {
        let profile: InflationProfile;
        if (type === "none") {
            profile = "None";
        } else if (type === "fixed") {
            profile = { Fixed: (fixed ?? 3.5) / 100 };
        } else {
            profile = { Normal: { mean: (mean ?? 3.5) / 100, std_dev: (stdDev ?? 2.8) / 100 } };
        }
        updateParameters("inflation_profile", profile);
    };

    // Inflation profile helpers
    const addInflationProfile = () => {
        const newProfile: NamedInflationProfile = {
            name: `Inflation ${namedInflationProfiles.length + 1}`,
            profile: { Normal: { mean: 0.035, std_dev: 0.028 } },
        };
        const updated = [...namedInflationProfiles, newProfile];
        setNamedInflationProfiles(updated);
        updateParameters("named_inflation_profiles", updated);
    };

    const updateInflationProfile = (index: number, updates: Partial<NamedInflationProfile>) => {
        const updated = namedInflationProfiles.map((p, i) => (i === index ? { ...p, ...updates } : p));
        setNamedInflationProfiles(updated);
        updateParameters("named_inflation_profiles", updated);
    };

    const removeInflationProfile = (index: number) => {
        const updated = namedInflationProfiles.filter((_, i) => i !== index);
        setNamedInflationProfiles(updated);
        updateParameters("named_inflation_profiles", updated);
    };

    const getInflationProfileValue = (profile: InflationProfile, key: "mean" | "std_dev" | "fixed"): number => {
        if (key === "fixed" && typeof profile === "object" && "Fixed" in profile) {
            return Math.round(profile.Fixed * 100 * 10) / 10;
        }
        if (typeof profile === "object" && "Normal" in profile) {
            const value = key === "mean" ? profile.Normal.mean * 100 : profile.Normal.std_dev * 100;
            return Math.round(value * 10) / 10;
        }
        return key === "mean" ? 3.5 : 2.8;
    };

    const getInflationProfileType = (profile: InflationProfile): string => {
        if (profile === "None") return "none";
        if (typeof profile === "object" && "Fixed" in profile) return "fixed";
        return "normal";
    };

    const updateInflationProfileValues = (index: number, type: string, mean?: number, stdDev?: number, fixed?: number) => {
        let profile: InflationProfile;
        if (type === "none") {
            profile = "None";
        } else if (type === "fixed") {
            profile = { Fixed: (fixed ?? 3.5) / 100 };
        } else {
            profile = { Normal: { mean: (mean ?? 3.5) / 100, std_dev: (stdDev ?? 2.8) / 100 } };
        }
        updateInflationProfile(index, { profile });
    };

    // Return profile helpers
    const addReturnProfile = () => {
        const newProfile: NamedReturnProfile = {
            name: `Profile ${namedProfiles.length + 1}`,
            profile: { Normal: { mean: 0.07, std_dev: 0.12 } },
        };
        const updated = [...namedProfiles, newProfile];
        setNamedProfiles(updated);
        updateParameters("named_return_profiles", updated);
        // Keep return_profiles in sync for backend compatibility
        updateParameters("return_profiles", updated.map((p) => p.profile));
    };

    const updateReturnProfile = (index: number, updates: Partial<NamedReturnProfile>) => {
        const updated = namedProfiles.map((p, i) => (i === index ? { ...p, ...updates } : p));
        setNamedProfiles(updated);
        updateParameters("named_return_profiles", updated);
        updateParameters("return_profiles", updated.map((p) => p.profile));
    };

    const removeReturnProfile = (index: number) => {
        const updated = namedProfiles.filter((_, i) => i !== index);
        setNamedProfiles(updated);
        updateParameters("named_return_profiles", updated);
        updateParameters("return_profiles", updated.map((p) => p.profile));
    };

    const getProfileValue = (profile: ReturnProfile, key: "mean" | "std_dev" | "fixed"): number => {
        if (key === "fixed" && typeof profile === "object" && "Fixed" in profile) {
            return Math.round(profile.Fixed * 100 * 10) / 10;
        }
        if (typeof profile === "object" && "Normal" in profile) {
            const value = key === "mean" ? profile.Normal.mean * 100 : profile.Normal.std_dev * 100;
            return Math.round(value * 10) / 10;
        }
        return key === "mean" ? 7.0 : 12.0;
    };

    const getProfileType = (profile: ReturnProfile): string => {
        if (profile === "None") return "none";
        if (typeof profile === "object" && "Fixed" in profile) return "fixed";
        return "normal";
    };

    const updateProfileValues = (index: number, type: string, mean?: number, stdDev?: number, fixed?: number) => {
        let profile: ReturnProfile;
        if (type === "none") {
            profile = "None";
        } else if (type === "fixed") {
            profile = { Fixed: (fixed ?? 5.0) / 100 };
        } else {
            profile = { Normal: { mean: (mean ?? 7.0) / 100, std_dev: (stdDev ?? 12.0) / 100 } };
        }
        updateReturnProfile(index, { profile });
    };

    return (
        <div className="space-y-8">
            {/* Default Inflation Profile */}
            <div className="space-y-4">
                <h3 className="text-lg font-medium">Default Inflation Profile</h3>
                <p className="text-sm text-muted-foreground">
                    This is the default inflation rate used for cash flows and general adjustments.
                </p>
                <RadioGroup
                    value={inflationType}
                    onValueChange={(v) => {
                        setInflationType(v);
                        updateInflation(v);
                    }}
                    className="grid grid-cols-3 gap-4"
                >
                    <div className="flex items-center space-x-2">
                        <RadioGroupItem value="none" id="inflation-none" />
                        <Label htmlFor="inflation-none">None</Label>
                    </div>
                    <div className="flex items-center space-x-2">
                        <RadioGroupItem value="fixed" id="inflation-fixed" />
                        <Label htmlFor="inflation-fixed">Fixed Rate</Label>
                    </div>
                    <div className="flex items-center space-x-2">
                        <RadioGroupItem value="normal" id="inflation-normal" />
                        <Label htmlFor="inflation-normal">Variable (Normal)</Label>
                    </div>
                </RadioGroup>

                {inflationType === "fixed" && (
                    <div className="space-y-2">
                        <Label>Annual Inflation Rate (%)</Label>
                        <Input
                            type="number"
                            step="0.1"
                            value={getInflationValue("fixed")}
                            onChange={(e) => updateInflation("fixed", undefined, undefined, parseFloat(e.target.value))}
                        />
                    </div>
                )}

                {inflationType === "normal" && (
                    <div className="grid gap-4 md:grid-cols-2">
                        <div className="space-y-2">
                            <Label>Mean (%)</Label>
                            <Input
                                type="number"
                                step="0.1"
                                value={getInflationValue("mean")}
                                onChange={(e) => updateInflation("normal", parseFloat(e.target.value), getInflationValue("std_dev"))}
                            />
                            <p className="text-xs text-muted-foreground">US historical: ~3.5%</p>
                        </div>
                        <div className="space-y-2">
                            <Label>Standard Deviation (%)</Label>
                            <Input
                                type="number"
                                step="0.1"
                                value={getInflationValue("std_dev")}
                                onChange={(e) => updateInflation("normal", getInflationValue("mean"), parseFloat(e.target.value))}
                            />
                            <p className="text-xs text-muted-foreground">US historical: ~2.8%</p>
                        </div>
                    </div>
                )}
            </div>

            {/* Named Inflation Profiles */}
            <div className="space-y-4">
                <div className="flex justify-between items-center">
                    <div>
                        <h3 className="text-lg font-medium">Inflation Profiles</h3>
                        <p className="text-sm text-muted-foreground">
                            Define different inflation profiles for various asset categories (e.g., healthcare, housing).
                            Assets can be linked to these in the next step.
                        </p>
                    </div>
                    <Button onClick={addInflationProfile} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Inflation Profile
                    </Button>
                </div>

                {namedInflationProfiles.length === 0 ? (
                    <Card className="border-dashed">
                        <CardContent className="flex flex-col items-center justify-center py-6">
                            <p className="text-muted-foreground mb-4">No custom inflation profiles defined</p>
                            <Button onClick={addInflationProfile} variant="outline" size="sm">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Inflation Profile
                            </Button>
                        </CardContent>
                    </Card>
                ) : (
                    <div className="space-y-3">
                        {namedInflationProfiles.map((namedProfile, index) => {
                            const profileType = getInflationProfileType(namedProfile.profile);
                            return (
                                <Card key={index} className="p-4">
                                    <div className="flex justify-between items-start mb-3">
                                        <Input
                                            value={namedProfile.name}
                                            onChange={(e) => updateInflationProfile(index, { name: e.target.value })}
                                            className="font-medium max-w-xs"
                                            placeholder="Profile Name"
                                        />
                                        <Button
                                            variant="ghost"
                                            size="icon"
                                            onClick={() => removeInflationProfile(index)}
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                    <div className="grid gap-3 md:grid-cols-3">
                                        <Select
                                            value={profileType}
                                            onValueChange={(v) => updateInflationProfileValues(index, v)}
                                        >
                                            <SelectTrigger className="h-8">
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                <SelectItem value="none">None</SelectItem>
                                                <SelectItem value="fixed">Fixed</SelectItem>
                                                <SelectItem value="normal">Variable</SelectItem>
                                            </SelectContent>
                                        </Select>
                                        {profileType === "fixed" && (
                                            <div className="flex items-center gap-2">
                                                <Label className="text-xs whitespace-nowrap">Rate (%)</Label>
                                                <Input
                                                    type="number"
                                                    step="0.1"
                                                    value={getInflationProfileValue(namedProfile.profile, "fixed")}
                                                    onChange={(e) => updateInflationProfileValues(index, "fixed", undefined, undefined, parseFloat(e.target.value))}
                                                    className="h-8"
                                                />
                                            </div>
                                        )}
                                        {profileType === "normal" && (
                                            <>
                                                <div className="flex items-center gap-2">
                                                    <Label className="text-xs whitespace-nowrap">Mean (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getInflationProfileValue(namedProfile.profile, "mean")}
                                                        onChange={(e) => updateInflationProfileValues(index, "normal", parseFloat(e.target.value), getInflationProfileValue(namedProfile.profile, "std_dev"))}
                                                        className="h-8"
                                                    />
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <Label className="text-xs whitespace-nowrap">Std Dev (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getInflationProfileValue(namedProfile.profile, "std_dev")}
                                                        onChange={(e) => updateInflationProfileValues(index, "normal", getInflationProfileValue(namedProfile.profile, "mean"), parseFloat(e.target.value))}
                                                        className="h-8"
                                                    />
                                                </div>
                                            </>
                                        )}
                                    </div>
                                </Card>
                            );
                        })}
                    </div>
                )}
            </div>

            {/* Named Return Profiles */}
            <div className="space-y-4">
                <div className="flex justify-between items-center">
                    <div>
                        <h3 className="text-lg font-medium">Return Profiles</h3>
                        <p className="text-sm text-muted-foreground">
                            Define return profiles that can be assigned to assets in your accounts.
                        </p>
                    </div>
                    <Button onClick={addReturnProfile} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Profile
                    </Button>
                </div>

                {namedProfiles.length === 0 ? (
                    <Card className="border-dashed">
                        <CardContent className="flex flex-col items-center justify-center py-8">
                            <p className="text-muted-foreground mb-4">No return profiles defined</p>
                            <Button onClick={addReturnProfile} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Return Profile
                            </Button>
                        </CardContent>
                    </Card>
                ) : (
                    <div className="space-y-4">
                        {namedProfiles.map((namedProfile, index) => {
                            const profileType = getProfileType(namedProfile.profile);
                            return (
                                <Card key={index}>
                                    <CardHeader className="pb-3">
                                        <div className="flex justify-between items-start">
                                            <div className="flex-1 mr-4">
                                                <Input
                                                    value={namedProfile.name}
                                                    onChange={(e) => updateReturnProfile(index, { name: e.target.value })}
                                                    className="font-medium"
                                                    placeholder="Profile Name"
                                                />
                                            </div>
                                            <Button
                                                variant="ghost"
                                                size="icon"
                                                onClick={() => removeReturnProfile(index)}
                                                disabled={namedProfiles.length === 1}
                                            >
                                                <Trash2 className="h-4 w-4" />
                                            </Button>
                                        </div>
                                    </CardHeader>
                                    <CardContent className="space-y-4">
                                        <RadioGroup
                                            value={profileType}
                                            onValueChange={(v) => updateProfileValues(index, v)}
                                            className="grid grid-cols-3 gap-4"
                                        >
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="none" id={`return-none-${index}`} />
                                                <Label htmlFor={`return-none-${index}`}>None</Label>
                                            </div>
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="fixed" id={`return-fixed-${index}`} />
                                                <Label htmlFor={`return-fixed-${index}`}>Fixed Rate</Label>
                                            </div>
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="normal" id={`return-normal-${index}`} />
                                                <Label htmlFor={`return-normal-${index}`}>Variable (Normal)</Label>
                                            </div>
                                        </RadioGroup>

                                        {profileType === "fixed" && (
                                            <div className="space-y-2">
                                                <Label>Annual Return Rate (%)</Label>
                                                <Input
                                                    type="number"
                                                    step="0.1"
                                                    value={getProfileValue(namedProfile.profile, "fixed")}
                                                    onChange={(e) => updateProfileValues(index, "fixed", undefined, undefined, parseFloat(e.target.value))}
                                                />
                                            </div>
                                        )}

                                        {profileType === "normal" && (
                                            <div className="grid gap-4 md:grid-cols-2">
                                                <div className="space-y-2">
                                                    <Label>Mean (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getProfileValue(namedProfile.profile, "mean")}
                                                        onChange={(e) => updateProfileValues(index, "normal", parseFloat(e.target.value), getProfileValue(namedProfile.profile, "std_dev"))}
                                                    />
                                                </div>
                                                <div className="space-y-2">
                                                    <Label>Standard Deviation (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getProfileValue(namedProfile.profile, "std_dev")}
                                                        onChange={(e) => updateProfileValues(index, "normal", getProfileValue(namedProfile.profile, "mean"), parseFloat(e.target.value))}
                                                    />
                                                </div>
                                            </div>
                                        )}
                                    </CardContent>
                                </Card>
                            );
                        })}
                    </div>
                )}
            </div>
        </div>
    );
}

// Asset Linking Step - Link portfolio assets to return and inflation profiles
function AssetLinkingStep({
    parameters,
    updateParameters,
    selectedPortfolio,
}: StepProps & { selectedPortfolio: SavedPortfolio | null }) {
    const namedReturnProfiles = parameters.named_return_profiles || DEFAULT_NAMED_RETURN_PROFILES;
    const namedInflationProfiles = parameters.named_inflation_profiles || DEFAULT_NAMED_INFLATION_PROFILES;
    const accounts = parameters.accounts || [];

    const [assetMappings, setAssetMappings] = React.useState<AssetInflationMapping[]>(
        parameters.asset_inflation_mappings || []
    );

    const getAssetInflationIndex = (accountId: number, assetId: number): number => {
        const mapping = assetMappings.find(m => m.account_id === accountId && m.asset_id === assetId);
        return mapping?.inflation_profile_index ?? 0;
    };

    const updateAssetInflationMapping = (accountId: number, assetId: number, inflationIndex: number) => {
        const existingIndex = assetMappings.findIndex(m => m.account_id === accountId && m.asset_id === assetId);
        let newMappings: AssetInflationMapping[];

        if (existingIndex >= 0) {
            newMappings = assetMappings.map((m, i) =>
                i === existingIndex ? { ...m, inflation_profile_index: inflationIndex } : m
            );
        } else {
            newMappings = [...assetMappings, { account_id: accountId, asset_id: assetId, inflation_profile_index: inflationIndex }];
        }

        setAssetMappings(newMappings);
        updateParameters("asset_inflation_mappings", newMappings);
    };

    const updateAssetReturnProfile = (accountIndex: number, assetIndex: number, returnIndex: number) => {
        const newAccounts = accounts.map((acc, i) => {
            if (i !== accountIndex) return acc;
            const newAssets = acc.assets.map((asset, j) =>
                j === assetIndex ? { ...asset, return_profile_index: returnIndex } : asset
            );
            return { ...acc, assets: newAssets };
        });
        updateParameters("accounts", newAccounts);
    };

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(amount);

    if (!selectedPortfolio && accounts.length === 0) {
        return (
            <div className="space-y-6">
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No portfolio selected</p>
                        <p className="text-sm text-muted-foreground text-center max-w-md">
                            Go back to the Basics step to select a portfolio, or create one first.
                        </p>
                    </CardContent>
                </Card>
            </div>
        );
    }

    return (
        <div className="space-y-6">
            <div className="bg-muted/50 rounded-lg p-4">
                <h3 className="font-medium mb-2">Link Assets to Profiles</h3>
                <p className="text-sm text-muted-foreground">
                    Assign return and inflation profiles to each asset in your portfolio.
                    This determines how each asset grows and how inflation affects its real value.
                </p>
            </div>

            {accounts.map((account, accountIndex) => (
                <Card key={account.account_id}>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-base">
                            {account.name || `Account #${account.account_id}`}
                            <span className="text-sm font-normal text-muted-foreground ml-2">
                                ({account.account_type})
                            </span>
                        </CardTitle>
                        <p className="text-sm text-muted-foreground">
                            {formatCurrency(account.assets.reduce((s, a) => s + a.initial_value, 0))}
                        </p>
                    </CardHeader>
                    <CardContent>
                        {account.assets.length === 0 ? (
                            <p className="text-sm text-muted-foreground">No assets in this account</p>
                        ) : (
                            <div className="space-y-3">
                                {account.assets.map((asset, assetIndex) => (
                                    <div
                                        key={asset.asset_id}
                                        className="grid gap-3 md:grid-cols-4 items-center p-3 bg-muted/30 rounded-lg"
                                    >
                                        <div>
                                            <p className="font-medium text-sm">{asset.name || `Asset #${asset.asset_id}`}</p>
                                            <p className="text-xs text-muted-foreground">
                                                {formatCurrency(asset.initial_value)} • {asset.asset_class}
                                            </p>
                                        </div>
                                        <div className="space-y-1">
                                            <Label className="text-xs">Return Profile</Label>
                                            <Select
                                                value={asset.return_profile_index.toString()}
                                                onValueChange={(v) => updateAssetReturnProfile(accountIndex, assetIndex, parseInt(v))}
                                            >
                                                <SelectTrigger className="h-8">
                                                    <SelectValue />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    {namedReturnProfiles.map((profile, idx) => (
                                                        <SelectItem key={idx} value={idx.toString()}>
                                                            {profile.name}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                        <div className="space-y-1">
                                            <Label className="text-xs">Inflation Profile</Label>
                                            <Select
                                                value={getAssetInflationIndex(account.account_id, asset.asset_id).toString()}
                                                onValueChange={(v) => updateAssetInflationMapping(account.account_id, asset.asset_id, parseInt(v))}
                                            >
                                                <SelectTrigger className="h-8">
                                                    <SelectValue />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    <SelectItem value="0">Default (General CPI)</SelectItem>
                                                    {namedInflationProfiles.map((profile, idx) => (
                                                        <SelectItem key={idx} value={(idx + 1).toString()}>
                                                            {profile.name}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                        <div className="text-right text-xs text-muted-foreground">
                                            ID: {asset.asset_id}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </CardContent>
                </Card>
            ))}
        </div>
    );
}

function AccountsStep({ parameters, updateParameters }: StepProps) {
    const [accounts, setAccounts] = React.useState<Account[]>(parameters.accounts || []);
    const namedProfiles = parameters.named_return_profiles || DEFAULT_NAMED_RETURN_PROFILES;

    const addAccount = () => {
        const newId = accounts.length > 0 ? Math.max(...accounts.map((a) => a.account_id)) + 1 : 1;
        const newAccount: Account = {
            account_id: newId,
            account_type: "Taxable",
            assets: [],
        };
        const updated = [...accounts, newAccount];
        setAccounts(updated);
        updateParameters("accounts", updated);
    };

    const updateAccount = (index: number, updates: Partial<Account>) => {
        const updated = accounts.map((acc, i) => (i === index ? { ...acc, ...updates } : acc));
        setAccounts(updated);
        updateParameters("accounts", updated);
    };

    const addAsset = (accountIndex: number) => {
        const account = accounts[accountIndex];
        const newAssetId = account.assets.length > 0
            ? Math.max(...account.assets.map((a) => a.asset_id)) + 1
            : account.account_id * 100;
        const newAsset: Asset = {
            asset_id: newAssetId,
            name: `Asset ${account.assets.length + 1}`,
            asset_class: "Investable",
            initial_value: 0,
            return_profile_index: 0,
        };
        const updatedAssets = [...account.assets, newAsset];
        updateAccount(accountIndex, { assets: updatedAssets });
    };

    const updateAsset = (accountIndex: number, assetIndex: number, updates: Partial<Asset>) => {
        const updated = accounts.map((acc, i) => {
            if (i !== accountIndex) return acc;
            const newAssets = acc.assets.map((asset, j) =>
                j === assetIndex ? { ...asset, ...updates } : asset
            );
            return { ...acc, assets: newAssets };
        });
        setAccounts(updated);
        updateParameters("accounts", updated);
    };

    const removeAsset = (accountIndex: number, assetIndex: number) => {
        const updated = accounts.map((acc, i) => {
            if (i !== accountIndex) return acc;
            return { ...acc, assets: acc.assets.filter((_, j) => j !== assetIndex) };
        });
        setAccounts(updated);
        updateParameters("accounts", updated);
    };

    const removeAccount = (index: number) => {
        const updated = accounts.filter((_, i) => i !== index);
        setAccounts(updated);
        updateParameters("accounts", updated);
    };

    const ACCOUNT_TYPES: { value: AccountType; label: string; description: string }[] = [
        { value: "Taxable", label: "Taxable", description: "Regular brokerage account" },
        { value: "TaxDeferred", label: "Tax-Deferred", description: "401(k), Traditional IRA" },
        { value: "TaxFree", label: "Tax-Free", description: "Roth IRA, Roth 401(k)" },
        { value: "Illiquid", label: "Illiquid", description: "Real estate, vehicles" },
    ];

    const ASSET_CLASSES: { value: AssetClass; label: string }[] = [
        { value: "Investable", label: "Investable" },
        { value: "RealEstate", label: "Real Estate" },
        { value: "Depreciating", label: "Depreciating" },
        { value: "Liability", label: "Liability (Debt)" },
    ];

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <p className="text-sm text-muted-foreground">
                    Add your financial accounts. Each account can hold multiple assets with different return profiles.
                </p>
                <Button onClick={addAccount} size="sm">
                    <Plus className="mr-2 h-4 w-4" />
                    Add Account
                </Button>
            </div>

            {accounts.length === 0 ? (
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No accounts added yet</p>
                        <Button onClick={addAccount} variant="outline">
                            <Plus className="mr-2 h-4 w-4" />
                            Add Your First Account
                        </Button>
                    </CardContent>
                </Card>
            ) : (
                <div className="space-y-4">
                    {accounts.map((account, accountIndex) => (
                        <Card key={account.account_id}>
                            <CardHeader className="pb-3">
                                <div className="flex justify-between items-start">
                                    <div className="space-y-1 flex-1">
                                        <div className="flex items-center gap-4">
                                            <CardTitle className="text-base">Account #{account.account_id}</CardTitle>
                                            <Select
                                                value={account.account_type}
                                                onValueChange={(v) => updateAccount(accountIndex, { account_type: v as AccountType })}
                                            >
                                                <SelectTrigger className="w-[180px]">
                                                    <SelectValue />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    {ACCOUNT_TYPES.map((type) => (
                                                        <SelectItem key={type.value} value={type.value}>
                                                            <div>
                                                                <span>{type.label}</span>
                                                                <span className="text-xs text-muted-foreground ml-2">
                                                                    ({type.description})
                                                                </span>
                                                            </div>
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                        <p className="text-sm text-muted-foreground">
                                            Total: ${account.assets.reduce((s, a) => s + (a.asset_class === "Liability" ? -a.initial_value : a.initial_value), 0).toLocaleString()}
                                        </p>
                                    </div>
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => removeAccount(accountIndex)}
                                    >
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                {/* Assets List */}
                                <div className="space-y-3">
                                    <div className="flex justify-between items-center">
                                        <Label className="text-sm font-medium">Assets</Label>
                                        <Button onClick={() => addAsset(accountIndex)} size="sm" variant="outline">
                                            <Plus className="mr-2 h-3 w-3" />
                                            Add Asset
                                        </Button>
                                    </div>

                                    {account.assets.length === 0 ? (
                                        <div className="border-2 border-dashed rounded-lg p-4 text-center">
                                            <p className="text-sm text-muted-foreground mb-2">No assets in this account</p>
                                            <Button onClick={() => addAsset(accountIndex)} size="sm" variant="ghost">
                                                <Plus className="mr-2 h-3 w-3" />
                                                Add First Asset
                                            </Button>
                                        </div>
                                    ) : (
                                        <div className="space-y-3">
                                            {account.assets.map((asset, assetIndex) => (
                                                <div
                                                    key={asset.asset_id}
                                                    className="border rounded-lg p-3 space-y-3 bg-muted/30"
                                                >
                                                    <div className="flex items-center gap-2">
                                                        <Input
                                                            value={asset.name || `Asset ${assetIndex + 1}`}
                                                            onChange={(e) => updateAsset(accountIndex, assetIndex, { name: e.target.value })}
                                                            className="flex-1 h-8"
                                                            placeholder="Asset Name"
                                                        />
                                                        <Button
                                                            variant="ghost"
                                                            size="icon"
                                                            className="h-8 w-8"
                                                            onClick={() => removeAsset(accountIndex, assetIndex)}
                                                        >
                                                            <Trash2 className="h-3 w-3" />
                                                        </Button>
                                                    </div>
                                                    <div className="grid gap-3 md:grid-cols-3">
                                                        <div className="space-y-1">
                                                            <Label className="text-xs">Value ($)</Label>
                                                            <MoneyInput
                                                                value={asset.initial_value}
                                                                onChange={(value) => updateAsset(accountIndex, assetIndex, { initial_value: value })}
                                                                className="h-8"
                                                            />
                                                        </div>
                                                        <div className="space-y-1">
                                                            <Label className="text-xs">Asset Class</Label>
                                                            <Select
                                                                value={asset.asset_class}
                                                                onValueChange={(v) => updateAsset(accountIndex, assetIndex, { asset_class: v as AssetClass })}
                                                            >
                                                                <SelectTrigger className="h-8">
                                                                    <SelectValue />
                                                                </SelectTrigger>
                                                                <SelectContent>
                                                                    {ASSET_CLASSES.map((ac) => (
                                                                        <SelectItem key={ac.value} value={ac.value}>
                                                                            {ac.label}
                                                                        </SelectItem>
                                                                    ))}
                                                                </SelectContent>
                                                            </Select>
                                                        </div>
                                                        <div className="space-y-1">
                                                            <Label className="text-xs">Return Profile</Label>
                                                            <Select
                                                                value={asset.return_profile_index.toString()}
                                                                onValueChange={(v) => updateAsset(accountIndex, assetIndex, { return_profile_index: parseInt(v) })}
                                                            >
                                                                <SelectTrigger className="h-8">
                                                                    <SelectValue />
                                                                </SelectTrigger>
                                                                <SelectContent>
                                                                    {namedProfiles.map((profile, idx) => (
                                                                        <SelectItem key={idx} value={idx.toString()}>
                                                                            {profile.name}
                                                                        </SelectItem>
                                                                    ))}
                                                                </SelectContent>
                                                            </Select>
                                                        </div>
                                                    </div>
                                                </div>
                                            ))}
                                        </div>
                                    )}
                                </div>
                            </CardContent>
                        </Card>
                    ))}
                </div>
            )}
        </div>
    );
}

function CashFlowsStep({ parameters, updateParameters, selectedPortfolio }: StepProps & { selectedPortfolio: SavedPortfolio | null }) {
    const [cashFlows, setCashFlows] = React.useState<CashFlow[]>(parameters.cash_flows || []);
    // Use portfolio accounts for names if available, otherwise fall back to parameters.accounts
    const accounts = selectedPortfolio?.accounts || parameters.accounts || [];

    // Build a list of account + asset options
    const accountAssetOptions = React.useMemo(() => {
        const options: { accountId: number; assetId: number; label: string }[] = [];
        accounts.forEach((acc) => {
            acc.assets.forEach((asset) => {
                const accountName = acc.name || `Account #${acc.account_id}`;
                const assetName = asset.name || `Asset #${asset.asset_id}`;
                options.push({
                    accountId: acc.account_id,
                    assetId: asset.asset_id,
                    label: `${accountName} → ${assetName}`,
                });
            });
        });
        return options;
    }, [accounts]);

    const getDefaultAccountAsset = () => {
        if (accountAssetOptions.length > 0) {
            return { accountId: accountAssetOptions[0].accountId, assetId: accountAssetOptions[0].assetId };
        }
        return { accountId: 1, assetId: 100 };
    };

    const addCashFlow = (type: "income" | "expense") => {
        const newId = cashFlows.length > 0 ? Math.max(...cashFlows.map((cf) => cf.cash_flow_id)) + 1 : 1;
        const defaults = getDefaultAccountAsset();
        const newCashFlow: CashFlow = {
            cash_flow_id: newId,
            amount: 0,
            repeats: "Monthly",
            adjust_for_inflation: true,
            direction: type === "income"
                ? { Income: { target_account_id: defaults.accountId, target_asset_id: defaults.assetId } }
                : { Expense: { source_account_id: defaults.accountId, source_asset_id: defaults.assetId } },
            state: "Active",
        };
        const updated = [...cashFlows, newCashFlow];
        setCashFlows(updated);
        updateParameters("cash_flows", updated);
    };

    const updateCashFlow = (index: number, updates: Partial<CashFlow>) => {
        const updated = cashFlows.map((cf, i) => (i === index ? { ...cf, ...updates } : cf));
        setCashFlows(updated);
        updateParameters("cash_flows", updated);
    };

    const removeCashFlow = (index: number) => {
        const updated = cashFlows.filter((_, i) => i !== index);
        setCashFlows(updated);
        updateParameters("cash_flows", updated);
    };

    const isIncome = (cf: CashFlow) => "Income" in cf.direction;

    const REPEAT_OPTIONS: { value: RepeatInterval; label: string }[] = [
        { value: "Never", label: "One-time" },
        { value: "Weekly", label: "Weekly" },
        { value: "BiWeekly", label: "Bi-weekly" },
        { value: "Monthly", label: "Monthly" },
        { value: "Quarterly", label: "Quarterly" },
        { value: "Yearly", label: "Yearly" },
    ];

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <p className="text-sm text-muted-foreground">
                    Define your income sources and regular expenses.
                </p>
                <div className="flex gap-2">
                    <Button onClick={() => addCashFlow("income")} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Income
                    </Button>
                    <Button onClick={() => addCashFlow("expense")} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Expense
                    </Button>
                </div>
            </div>

            {cashFlows.length === 0 ? (
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No cash flows defined yet</p>
                        <div className="flex gap-2">
                            <Button onClick={() => addCashFlow("income")} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Income
                            </Button>
                            <Button onClick={() => addCashFlow("expense")} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Expense
                            </Button>
                        </div>
                    </CardContent>
                </Card>
            ) : (
                <div className="space-y-4">
                    {cashFlows.map((cf, index) => (
                        <Card key={cf.cash_flow_id}>
                            <CardHeader className="pb-3">
                                <div className="flex justify-between items-start">
                                    <CardTitle className="text-base">
                                        {isIncome(cf) ? "💰 Income" : "💸 Expense"} #{cf.cash_flow_id}
                                    </CardTitle>
                                    <Button variant="ghost" size="icon" onClick={() => removeCashFlow(index)}>
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div className="grid gap-4 md:grid-cols-3">
                                    <div className="space-y-2">
                                        <Label>Amount ($)</Label>
                                        <MoneyInput
                                            value={cf.amount}
                                            onChange={(value) => updateCashFlow(index, { amount: value })}
                                        />
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Frequency</Label>
                                        <Select
                                            value={cf.repeats}
                                            onValueChange={(v) => updateCashFlow(index, { repeats: v as RepeatInterval })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {REPEAT_OPTIONS.map((opt) => (
                                                    <SelectItem key={opt.value} value={opt.value}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Adjust for Inflation</Label>
                                        <Select
                                            value={cf.adjust_for_inflation ? "yes" : "no"}
                                            onValueChange={(v) => updateCashFlow(index, { adjust_for_inflation: v === "yes" })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                <SelectItem value="yes">Yes</SelectItem>
                                                <SelectItem value="no">No</SelectItem>
                                            </SelectContent>
                                        </Select>
                                    </div>
                                </div>
                                {isIncome(cf) && accountAssetOptions.length > 0 && (
                                    <div className="space-y-2">
                                        <Label>Deposit to Account / Asset</Label>
                                        <Select
                                            value={
                                                "Income" in cf.direction
                                                    ? `${cf.direction.Income.target_account_id}-${cf.direction.Income.target_asset_id}`
                                                    : accountAssetOptions[0] ? `${accountAssetOptions[0].accountId}-${accountAssetOptions[0].assetId}` : ""
                                            }
                                            onValueChange={(v) => {
                                                const [accountId, assetId] = v.split("-").map(Number);
                                                updateCashFlow(index, {
                                                    direction: { Income: { target_account_id: accountId, target_asset_id: assetId } },
                                                });
                                            }}
                                        >
                                            <SelectTrigger>
                                                <SelectValue placeholder="Select account & asset" />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {accountAssetOptions.map((opt) => (
                                                    <SelectItem key={`${opt.accountId}-${opt.assetId}`} value={`${opt.accountId}-${opt.assetId}`}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                )}
                                {!isIncome(cf) && accountAssetOptions.length > 0 && (
                                    <div className="space-y-2">
                                        <Label>Withdraw from Account / Asset</Label>
                                        <Select
                                            value={
                                                "Expense" in cf.direction
                                                    ? `${cf.direction.Expense.source_account_id}-${cf.direction.Expense.source_asset_id}`
                                                    : accountAssetOptions[0] ? `${accountAssetOptions[0].accountId}-${accountAssetOptions[0].assetId}` : ""
                                            }
                                            onValueChange={(v) => {
                                                const [accountId, assetId] = v.split("-").map(Number);
                                                updateCashFlow(index, {
                                                    direction: { Expense: { source_account_id: accountId, source_asset_id: assetId } },
                                                });
                                            }}
                                        >
                                            <SelectTrigger>
                                                <SelectValue placeholder="Select account & asset" />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {accountAssetOptions.map((opt) => (
                                                    <SelectItem key={`${opt.accountId}-${opt.assetId}`} value={`${opt.accountId}-${opt.assetId}`}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                )}
                            </CardContent>
                        </Card>
                    ))}
                </div>
            )}
        </div>
    );
}

function EventsStep({ parameters, updateParameters }: StepProps) {
    const [events, setEvents] = React.useState<Event[]>(parameters.events || []);

    // Events are complex - for now, show a simplified interface
    return (
        <div className="space-y-6">
            <div className="bg-muted/50 rounded-lg p-6 text-center">
                <h3 className="text-lg font-medium mb-2">Life Events</h3>
                <p className="text-sm text-muted-foreground mb-4">
                    Events allow you to model life changes like retirement, Social Security, or major purchases.
                    This is an advanced feature.
                </p>
                <p className="text-sm text-muted-foreground">
                    Events configured: <strong>{events.length}</strong>
                </p>
                <p className="text-xs text-muted-foreground mt-4">
                    Advanced event configuration coming soon. For now, events can be added via JSON import.
                </p>
            </div>
        </div>
    );
}

function SpendingStep({ parameters, updateParameters }: StepProps) {
    const [targets, setTargets] = React.useState<SpendingTarget[]>(parameters.spending_targets || []);

    const addTarget = () => {
        const newId = targets.length > 0 ? Math.max(...targets.map((t) => t.spending_target_id)) + 1 : 1;
        const newTarget: SpendingTarget = {
            spending_target_id: newId,
            amount: 0,
            net_amount_mode: false,
            repeats: "Monthly",
            adjust_for_inflation: true,
            withdrawal_strategy: "TaxOptimized",
            exclude_accounts: [],
            state: "Pending",
        };
        const updated = [...targets, newTarget];
        setTargets(updated);
        updateParameters("spending_targets", updated);
    };

    const updateTarget = (index: number, updates: Partial<SpendingTarget>) => {
        const updated = targets.map((t, i) => (i === index ? { ...t, ...updates } : t));
        setTargets(updated);
        updateParameters("spending_targets", updated);
    };

    const removeTarget = (index: number) => {
        const updated = targets.filter((_, i) => i !== index);
        setTargets(updated);
        updateParameters("spending_targets", updated);
    };

    const STRATEGY_OPTIONS: { value: string; label: string; description: string }[] = [
        { value: "TaxOptimized", label: "Tax Optimized", description: "Minimize taxes by withdrawing from taxable accounts first" },
        { value: "ProRata", label: "Pro-Rata", description: "Withdraw proportionally from all accounts" },
    ];

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <p className="text-sm text-muted-foreground">
                    Define spending targets for retirement. These determine how much you&apos;ll withdraw from accounts.
                </p>
                <Button onClick={addTarget} size="sm">
                    <Plus className="mr-2 h-4 w-4" />
                    Add Spending Target
                </Button>
            </div>

            {targets.length === 0 ? (
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No spending targets defined</p>
                        <p className="text-xs text-muted-foreground mb-4">
                            Spending targets are used during retirement to model withdrawals from your accounts.
                        </p>
                        <Button onClick={addTarget} variant="outline">
                            <Plus className="mr-2 h-4 w-4" />
                            Add Spending Target
                        </Button>
                    </CardContent>
                </Card>
            ) : (
                <div className="space-y-4">
                    {targets.map((target, index) => (
                        <Card key={target.spending_target_id}>
                            <CardHeader className="pb-3">
                                <div className="flex justify-between items-start">
                                    <CardTitle className="text-base">
                                        Spending Target #{target.spending_target_id}
                                    </CardTitle>
                                    <Button variant="ghost" size="icon" onClick={() => removeTarget(index)}>
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div className="grid gap-4 md:grid-cols-3">
                                    <div className="space-y-2">
                                        <Label>Amount ($)</Label>
                                        <MoneyInput
                                            value={target.amount}
                                            onChange={(value) => updateTarget(index, { amount: value })}
                                        />
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Frequency</Label>
                                        <Select
                                            value={target.repeats}
                                            onValueChange={(v) => updateTarget(index, { repeats: v as RepeatInterval })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                <SelectItem value="Monthly">Monthly</SelectItem>
                                                <SelectItem value="Yearly">Yearly</SelectItem>
                                            </SelectContent>
                                        </Select>
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Withdrawal Strategy</Label>
                                        <Select
                                            value={typeof target.withdrawal_strategy === "string" ? target.withdrawal_strategy : "Sequential"}
                                            onValueChange={(v) => updateTarget(index, { withdrawal_strategy: v as WithdrawalStrategy })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {STRATEGY_OPTIONS.map((opt) => (
                                                    <SelectItem key={opt.value} value={opt.value}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                </div>
                                <div className="flex items-center gap-4">
                                    <div className="flex items-center space-x-2">
                                        <input
                                            type="checkbox"
                                            id={`inflation-${target.spending_target_id}`}
                                            checked={target.adjust_for_inflation}
                                            onChange={(e) => updateTarget(index, { adjust_for_inflation: e.target.checked })}
                                            className="rounded border-input"
                                        />
                                        <Label htmlFor={`inflation-${target.spending_target_id}`}>Adjust for inflation</Label>
                                    </div>
                                    <div className="flex items-center space-x-2">
                                        <input
                                            type="checkbox"
                                            id={`net-${target.spending_target_id}`}
                                            checked={target.net_amount_mode}
                                            onChange={(e) => updateTarget(index, { net_amount_mode: e.target.checked })}
                                            className="rounded border-input"
                                        />
                                        <Label htmlFor={`net-${target.spending_target_id}`}>Net amount (after taxes)</Label>
                                    </div>
                                </div>
                            </CardContent>
                        </Card>
                    ))}
                </div>
            )}
        </div>
    );
}

function ReviewStep({
    name,
    description,
    parameters,
    selectedPortfolio,
}: {
    name: string;
    description: string;
    parameters: SimulationParameters;
    selectedPortfolio: SavedPortfolio | null;
}) {
    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(amount);

    const totalAccountValue = parameters.accounts.reduce(
        (sum, acc) => sum + acc.assets.reduce((s, a) => s + a.initial_value, 0),
        0
    );

    const monthlyIncome = parameters.cash_flows
        .filter((cf) => "Income" in cf.direction)
        .reduce((sum, cf) => {
            const multiplier = cf.repeats === "Monthly" ? 1 : cf.repeats === "Yearly" ? 1 / 12 : cf.repeats === "Weekly" ? 4.33 : cf.repeats === "BiWeekly" ? 2.17 : 0;
            return sum + cf.amount * multiplier;
        }, 0);

    const monthlyExpenses = parameters.cash_flows
        .filter((cf) => "Expense" in cf.direction)
        .reduce((sum, cf) => {
            const multiplier = cf.repeats === "Monthly" ? 1 : cf.repeats === "Yearly" ? 1 / 12 : cf.repeats === "Weekly" ? 4.33 : cf.repeats === "BiWeekly" ? 2.17 : 0;
            return sum + cf.amount * multiplier;
        }, 0);

    return (
        <div className="space-y-6">
            {!name && (
                <div className="bg-destructive/10 border border-destructive/20 rounded-lg p-4">
                    <p className="text-sm text-destructive">Please provide a name for your simulation before saving.</p>
                </div>
            )}

            <div className="grid gap-4 md:grid-cols-2">
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Simulation Name</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{name || "Untitled"}</p>
                        {description && <p className="text-sm text-muted-foreground mt-1">{description}</p>}
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Portfolio</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{selectedPortfolio?.name || "None"}</p>
                        <p className="text-sm text-muted-foreground">
                            {selectedPortfolio ? `${selectedPortfolio.accounts.length} accounts` : "No portfolio linked"}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Duration</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{parameters.duration_years} years</p>
                        <p className="text-sm text-muted-foreground">
                            {parameters.start_date ? `Starting ${parameters.start_date}` : "Starting today"}
                            {parameters.retirement_age && ` • Retire at ${parameters.retirement_age}`}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Total Account Value</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{formatCurrency(totalAccountValue)}</p>
                        <p className="text-sm text-muted-foreground">
                            Across {parameters.accounts.length} account{parameters.accounts.length !== 1 ? "s" : ""}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Monthly Cash Flow</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">
                            {formatCurrency(monthlyIncome - monthlyExpenses)}
                            <span className="text-sm font-normal text-muted-foreground">/mo</span>
                        </p>
                        <p className="text-sm text-muted-foreground">
                            {formatCurrency(monthlyIncome)} income - {formatCurrency(monthlyExpenses)} expenses
                        </p>
                    </CardContent>
                </Card>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle className="text-sm font-medium">Summary</CardTitle>
                </CardHeader>
                <CardContent>
                    <dl className="space-y-2 text-sm">
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Accounts</dt>
                            <dd>{parameters.accounts.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Cash Flows</dt>
                            <dd>{parameters.cash_flows.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Events</dt>
                            <dd>{parameters.events.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Spending Targets</dt>
                            <dd>{parameters.spending_targets.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Inflation Profile</dt>
                            <dd>
                                {parameters.inflation_profile === "None"
                                    ? "None"
                                    : typeof parameters.inflation_profile === "object" && "Fixed" in parameters.inflation_profile
                                        ? `Fixed ${(parameters.inflation_profile.Fixed * 100).toFixed(1)}%`
                                        : typeof parameters.inflation_profile === "object" && "Normal" in parameters.inflation_profile
                                            ? `Normal (μ=${(parameters.inflation_profile.Normal.mean * 100).toFixed(1)}%)`
                                            : "Unknown"}
                            </dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Return Profiles</dt>
                            <dd>
                                {parameters.named_return_profiles && parameters.named_return_profiles.length > 0
                                    ? `${parameters.named_return_profiles.length} profile${parameters.named_return_profiles.length !== 1 ? "s" : ""}`
                                    : "None"}
                            </dd>
                        </div>
                    </dl>
                </CardContent>
            </Card>
        </div>
    );
}
