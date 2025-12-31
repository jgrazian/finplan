"use client";

import * as React from "react";
import { Check } from "lucide-react";
import { cn } from "@/lib/utils";

interface Step {
    id: string;
    title: string;
    description?: string;
}

interface WizardProps {
    steps: Step[];
    currentStep: number;
    onStepClick?: (stepIndex: number) => void;
    allowNavigation?: boolean;
}

export function Wizard({
    steps,
    currentStep,
    onStepClick,
    allowNavigation = false,
}: WizardProps) {
    return (
        <nav aria-label="Progress">
            <ol className="flex items-center">
                {steps.map((step, stepIdx) => (
                    <li
                        key={step.id}
                        className={cn(
                            stepIdx !== steps.length - 1 ? "pr-8 sm:pr-20 flex-1" : "",
                            "relative"
                        )}
                    >
                        {stepIdx < currentStep ? (
                            // Completed step
                            <>
                                <div className="absolute inset-0 flex items-center" aria-hidden="true">
                                    <div className="h-0.5 w-full bg-primary" />
                                </div>
                                <button
                                    type="button"
                                    onClick={() => allowNavigation && onStepClick?.(stepIdx)}
                                    disabled={!allowNavigation}
                                    className={cn(
                                        "relative flex h-8 w-8 items-center justify-center rounded-full bg-primary hover:bg-primary/90",
                                        !allowNavigation && "cursor-default"
                                    )}
                                >
                                    <Check className="h-5 w-5 text-primary-foreground" aria-hidden="true" />
                                    <span className="sr-only">{step.title}</span>
                                </button>
                            </>
                        ) : stepIdx === currentStep ? (
                            // Current step
                            <>
                                <div className="absolute inset-0 flex items-center" aria-hidden="true">
                                    <div className="h-0.5 w-full bg-muted" />
                                </div>
                                <button
                                    type="button"
                                    className="relative flex h-8 w-8 items-center justify-center rounded-full border-2 border-primary bg-background cursor-default"
                                    aria-current="step"
                                >
                                    <span className="h-2.5 w-2.5 rounded-full bg-primary" aria-hidden="true" />
                                    <span className="sr-only">{step.title}</span>
                                </button>
                            </>
                        ) : (
                            // Upcoming step
                            <>
                                <div className="absolute inset-0 flex items-center" aria-hidden="true">
                                    <div className="h-0.5 w-full bg-muted" />
                                </div>
                                <button
                                    type="button"
                                    onClick={() => allowNavigation && onStepClick?.(stepIdx)}
                                    disabled={!allowNavigation}
                                    className={cn(
                                        "relative flex h-8 w-8 items-center justify-center rounded-full border-2 border-muted bg-background",
                                        allowNavigation ? "hover:border-muted-foreground" : "cursor-default"
                                    )}
                                >
                                    <span className="h-2.5 w-2.5 rounded-full bg-transparent" aria-hidden="true" />
                                    <span className="sr-only">{step.title}</span>
                                </button>
                            </>
                        )}
                        <div className="mt-2 hidden sm:block">
                            <span className="text-xs font-medium">{step.title}</span>
                            {step.description && (
                                <span className="text-xs text-muted-foreground block">{step.description}</span>
                            )}
                        </div>
                    </li>
                ))}
            </ol>
        </nav>
    );
}
