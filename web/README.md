# FinPlan Web Frontend

A professional Next.js frontend for the FinPlan Monte Carlo financial planning simulator.

## Features

- **Portfolio Management**: Configure multiple accounts with different asset types, initial balances, and return profiles
- **Cash Flow Configuration**: Set up recurring income and expenses with flexible timing and frequency options
- **Simulation Parameters**: Customize Monte Carlo simulation settings including duration and inflation profiles
- **Visual Results**: View simulation results as interactive bar charts broken down by account

## Technology Stack

- **Next.js 15** - React framework with App Router
- **TypeScript** - Type-safe development
- **Tailwind CSS** - Utility-first styling
- **shadcn/ui** - High-quality UI components
- **Recharts** - Data visualization
- **pnpm** - Fast, disk space efficient package manager

## Getting Started

### Prerequisites

- Node.js 18+ 
- pnpm

### Installation

```bash
pnpm install
```

### Development

```bash
pnpm dev
```

Open [http://localhost:3000](http://localhost:3000) to view the application.

### Build

```bash
pnpm build
pnpm start
```

## Backend API

The frontend expects a backend API running at `http://localhost:3000/api/simulate` that accepts POST requests with simulation parameters and returns aggregated results.

## Project Structure

```
src/
├── app/
│   ├── layout.tsx          # Root layout with fonts and metadata
│   ├── page.tsx            # Main application page
│   └── globals.css         # Global styles and Tailwind config
├── components/
│   ├── ui/                 # shadcn/ui base components
│   │   ├── button.tsx
│   │   ├── card.tsx
│   │   ├── input.tsx
│   │   ├── label.tsx
│   │   ├── select.tsx
│   │   └── tabs.tsx
│   ├── portfolio-editor.tsx        # Portfolio configuration UI
│   ├── simulation-parameters.tsx   # Simulation settings UI
│   └── simulation-results.tsx      # Results visualization
├── lib/
│   └── utils.ts            # Utility functions
└── types.ts                # TypeScript type definitions
```

## Usage

1. **Configure Portfolio**: Add accounts, set initial balances, and configure cash flows
2. **Set Parameters**: Adjust simulation duration and inflation settings
3. **Run Simulation**: Click "Run Monte Carlo Simulation" to execute
4. **View Results**: See projected portfolio values broken down by account

## License

See parent project for license information.
