import { SimulationParameters, SavedPortfolio, PortfolioListItem } from "@/lib/types";

export interface StepProps {
    parameters: SimulationParameters;
    updateParameters: <K extends keyof SimulationParameters>(key: K, value: SimulationParameters[K]) => void;
}

export interface SimulationWizardProps {
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

export interface SimulationFormState {
    name: string;
    description: string;
    parameters: SimulationParameters;
    selectedPortfolioId?: string;
    selectedPortfolio: SavedPortfolio | null;
}

export const WIZARD_STEPS = [
    { id: "basics", title: "Basics", description: "Name, dates & portfolio" },
    { id: "profiles", title: "Profiles", description: "Inflation & returns" },
    { id: "asset-linking", title: "Asset Linking", description: "Link assets to profiles" },
    { id: "cashflows", title: "Cash Flows", description: "Income & expenses" },
    { id: "events", title: "Events", description: "Life events" },
    { id: "spending", title: "Spending", description: "Retirement spending" },
    { id: "review", title: "Review", description: "Final review" },
];
