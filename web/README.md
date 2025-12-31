# FinPlan Web

A professional web frontend for the FinPlan Monte Carlo financial simulation engine.

## Features

- **Multi-step Simulation Wizard**: Create simulations step-by-step with an intuitive interface
  - Basic settings (name, duration, dates)
  - Inflation and return profiles
  - Financial accounts (Taxable, Tax-Deferred, Tax-Free, Illiquid)
  - Cash flows (income and expenses)
  - Life events
  - Retirement spending targets

- **Simulation Management**: Save, load, edit, and delete simulation configurations
  - SQLite-backed persistence via the backend API
  - View simulation details and parameters

- **Monte Carlo Results**: Visualize simulation outcomes
  - Interactive area charts showing 10th, 50th, and 90th percentile projections
  - Detailed data tables with yearly breakdowns
  - Per-account analysis

- **Run History**: Track all simulation runs with timestamps and iteration counts

## Getting Started

### Prerequisites

- Node.js 18+
- pnpm
- Running `finplan_server` backend on port 3001

### Installation

```bash
# Install dependencies
pnpm install

# Copy environment configuration
cp .env.example .env.local
```

### Development

```bash
pnpm dev
```

Open [http://localhost:3000](http://localhost:3000) with your browser.

### Production Build

```bash
# Build for production
pnpm build

# Start production server
pnpm start
```

## Backend API

The web app expects the `finplan_server` to be running at the URL specified in `NEXT_PUBLIC_API_URL` (defaults to `http://localhost:3001`).

Start the backend:

```bash
cd ../crates/finplan_server
cargo run --release
```

## Project Structure

```
web/
├── app/                    # Next.js App Router pages
│   ├── page.tsx           # Dashboard
│   ├── simulations/       # Simulation pages
│   │   ├── page.tsx       # List all simulations
│   │   ├── new/           # Create new simulation
│   │   └── [id]/          # View/edit simulation
│   └── results/           # Results pages
│       ├── page.tsx       # Results overview
│       └── history/       # Run history
├── components/            # React components
│   ├── simulation-wizard.tsx    # Multi-step form
│   ├── simulation-detail.tsx    # Simulation view
│   ├── simulations-list.tsx     # List component
│   ├── results-dashboard.tsx    # Charts and tables
│   └── ui/                      # shadcn/ui components
├── lib/
│   ├── api.ts            # API client functions
│   ├── types.ts          # TypeScript type definitions
│   └── utils.ts          # Utility functions
└── hooks/                # Custom React hooks
```

## Technology Stack

- **Framework**: Next.js 16 with App Router
- **Styling**: Tailwind CSS 4
- **UI Components**: shadcn/ui (New York style)
- **Charts**: Recharts
- **Tables**: TanStack Table
- **Forms**: React Hook Form with Zod validation
- **Date Handling**: date-fns
