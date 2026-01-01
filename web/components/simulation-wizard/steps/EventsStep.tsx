"use client";

import * as React from "react";
import { SimulationParameters, Event } from "@/lib/types";

interface StepProps {
    parameters: SimulationParameters;
    updateParameters: <K extends keyof SimulationParameters>(key: K, value: SimulationParameters[K]) => void;
}

export function EventsStep({ parameters, updateParameters }: StepProps) {
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
