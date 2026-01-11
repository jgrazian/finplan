"use client";

import * as React from "react";
import { Button } from "@/components/ui/button";
import { ArrowLeft, ArrowRight, Save } from "lucide-react";
import { useWizardStore } from "./hooks/useWizardStore";
import { WizardProgress } from "./components/WizardProgress";
import { WelcomeStep } from "./steps/WelcomeStep";
import { AboutYouStep } from "./steps/AboutYouStep";
import { CurrentIncomeStep } from "./steps/CurrentIncomeStep";
import { CurrentSavingsStep } from "./steps/CurrentSavingsStep";
import { InvestmentsStep } from "./steps/InvestmentsStep";
import { RealEstateStep } from "./steps/RealEstateStep";
import { DebtsStep } from "./steps/DebtsStep";
import { RetirementGoalsStep } from "./steps/RetirementGoalsStep";
import { LifeEventsStep } from "./steps/LifeEventsStep";
import { ReviewStep } from "./steps/ReviewStep";

const TOTAL_STEPS = 10;

export function SimulationWizardV2() {
    const currentStep = useWizardStore((state) => state.currentStep);
    const simulationName = useWizardStore((state) => state.simulationName);
    const personalInfo = useWizardStore((state) => state.personalInfo);
    const nextStep = useWizardStore((state) => state.nextStep);
    const prevStep = useWizardStore((state) => state.prevStep);
    const markStepComplete = useWizardStore((state) => state.markStepComplete);

    const canProceed = React.useMemo(() => {
        switch (currentStep) {
            case 0: // Welcome
                return simulationName.trim().length > 0;
            case 1: // About You
                return personalInfo.birthDate !== null &&
                    personalInfo.filingStatus !== null &&
                    personalInfo.state !== null;
            case 2: // Current Income
                // Can proceed with or without income
                return true;
            case 3: // Current Savings
                // Can proceed with or without savings
                return true;
            default:
                return true;
        }
    }, [currentStep, simulationName, personalInfo]);

    const handleNext = () => {
        markStepComplete(currentStep);
        nextStep();
    };

    const handlePrev = () => {
        prevStep();
    };

    const renderStep = () => {
        switch (currentStep) {
            case 0:
                return <WelcomeStep />;
            case 1:
                return <AboutYouStep />;
            case 2:
                return <CurrentIncomeStep />;
            case 3:
                return <CurrentSavingsStep />;
            case 4:
                return <InvestmentsStep />;
            case 5:
                return <RealEstateStep />;
            case 6:
                return <DebtsStep />;
            case 7:
                return <RetirementGoalsStep />;
            case 8:
                return <LifeEventsStep />;
            case 9:
                return <ReviewStep />;
            default:
                return null;
        }
    };

    return (
        <div className="flex h-full">
            {/* Progress Sidebar */}
            <WizardProgress />

            {/* Main Content */}
            <div className="flex-1 flex flex-col">
                {/* Content Area */}
                <div className="flex-1 overflow-y-auto p-8">
                    {renderStep()}
                </div>

                {/* Navigation Footer */}
                <div className="border-t bg-background p-4">
                    <div className="max-w-2xl mx-auto flex items-center justify-between">
                        <Button
                            variant="outline"
                            onClick={handlePrev}
                            disabled={currentStep === 0}
                        >
                            <ArrowLeft className="h-4 w-4 mr-2" />
                            Back
                        </Button>

                        <div className="text-sm text-muted-foreground">
                            Step {currentStep + 1} of {TOTAL_STEPS}
                        </div>

                        <div className="flex gap-2">
                            <Button variant="outline" size="icon">
                                <Save className="h-4 w-4" />
                            </Button>

                            {currentStep < TOTAL_STEPS - 1 ? (
                                <Button onClick={handleNext} disabled={!canProceed}>
                                    Continue
                                    <ArrowRight className="h-4 w-4 ml-2" />
                                </Button>
                            ) : (
                                <Button onClick={handleNext}>
                                    Run Simulation
                                    <ArrowRight className="h-4 w-4 ml-2" />
                                </Button>
                            )}
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
