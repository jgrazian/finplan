"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Wizard } from "@/components/ui/wizard";
import { ArrowRight, ArrowLeft, Save } from "lucide-react";
import {
    SimulationParameters,
    PortfolioListItem,
    SavedPortfolio,
    DEFAULT_SIMULATION_PARAMETERS,
} from "@/lib/types";
import { createSimulation, updateSimulation, listPortfolios, getPortfolio } from "@/lib/api";
import { BasicsStep } from "./simulation-wizard/steps/BasicsStep";
import { ProfilesStep } from "./simulation-wizard/steps/ProfilesStep";
import { AssetLinkingStep } from "./simulation-wizard/steps/AssetLinkingStep";
import { CashFlowsStep } from "./simulation-wizard/steps/CashFlowsStep";
import { EventsStep } from "./simulation-wizard/steps/EventsStep";
import { SpendingStep } from "./simulation-wizard/steps/SpendingStep";
import { ReviewStep } from "./simulation-wizard/steps/ReviewStep";

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
