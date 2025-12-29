import { useState } from 'react'
import { ResponsiveLine } from '@nivo/line'
import { SimulationParameters, AggregatedResult } from './types';

const defaultParams: SimulationParameters = {
    duration_years: 30,
    inflation_profile: { Fixed: 0.03 },
    events: [],
    accounts: [
        {
            account_id: 1,
            name: "Savings",
            initial_balance: 10000,
            account_type: "Taxable",
            return_profile: {
                Normal: {
                    mean: 0.079088,
                    std_dev: 0.161832,
                }
            },
            cash_flows: [
                {
                    cash_flow_id: 1,
                    description: "Savings",
                    amount: 1000,
                    start: "Immediate",
                    end: "Never",
                    repeats: "Monthly",
                    adjust_for_inflation: false,
                    cash_flow_limits: undefined
                }
            ]
        }
    ]
};

const currencyFormatter = (value: number | string | Date) => {
    if (typeof value !== 'number') return String(value);
    return new Intl.NumberFormat('en-US', {
        style: 'currency',
        currency: 'USD',
        minimumFractionDigits: 0,
        maximumFractionDigits: 1
    }).format(value / 1000) + 'k';
};

function App() {
    const [result, setResult] = useState<AggregatedResult | null>(null);
    const [loading, setLoading] = useState(false);

    const runSimulation = async () => {
        setLoading(true);
        try {
            const response = await fetch('http://localhost:3000/api/simulate', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                },
                body: JSON.stringify(defaultParams),
            });
            const data = await response.json();
            setResult(data);
        } catch (e) {
            console.error(e);
        } finally {
            setLoading(false);
        }
    };

    const chartData = result ? [
        {
            id: "90th Percentile",
            color: "hsl(45, 70%, 50%)",
            data: result.total_portfolio.map(p => ({ x: p.date, y: Math.max(1, p.p90) }))
        },
        {
            id: "Median",
            color: "hsl(140, 70%, 50%)",
            data: result.total_portfolio.map(p => ({ x: p.date, y: Math.max(1, p.p50) }))
        },
        {
            id: "10th Percentile",
            color: "hsl(260, 70%, 50%)",
            data: result.total_portfolio.map(p => ({ x: p.date, y: Math.max(1, p.p10) }))
        }
    ] : [];

    return (
        <div style={{ width: '100%', height: '100vh', padding: '20px', display: 'flex', flexDirection: 'column' }}>
            <h1>FinPlan</h1>
            <div style={{ marginBottom: '20px' }}>
                <button onClick={runSimulation} disabled={loading}>
                    {loading ? 'Running...' : 'Run Simulation'}
                </button>
            </div>

            {result && (
                <div style={{ flex: 1, minHeight: '600px', minWidth: '1200px' }}>
                    <h2>Total Portfolio Projection</h2>
                    <ResponsiveLine
                        data={chartData}
                        margin={{ top: 50, right: 110, bottom: 50, left: 80 }}
                        xScale={{
                            type: 'time',
                            format: '%Y-%m-%d',
                            precision: 'day',
                        }}
                        xFormat="time:%Y-%m-%d"
                        yScale={{
                            type: 'linear',
                            min: 0.0,
                            max: 'auto',
                        }}
                        axisTop={null}
                        axisRight={null}
                        axisBottom={{
                            format: '%Y-%m-%d',
                            tickValues: 'every 5 years',
                            tickSize: 5,
                            tickPadding: 5,
                            tickRotation: 0,
                            legend: 'Date',
                            legendOffset: 36,
                            legendPosition: 'middle'
                        }}
                        axisLeft={{
                            tickSize: 5,
                            tickPadding: 5,
                            tickRotation: 0,
                            legend: 'Portfolio Value (Log Scale)',
                            legendOffset: -65,
                            legendPosition: 'middle',
                            format: currencyFormatter
                        }}
                        pointSize={0}
                        pointBorderWidth={2}
                        pointBorderColor={{ from: 'serieColor' }}
                        pointLabelYOffset={-12}
                        useMesh={true}
                        legends={[
                            {
                                anchor: 'bottom-right',
                                direction: 'column',
                                justify: false,
                                translateX: 100,
                                translateY: 0,
                                itemsSpacing: 0,
                                itemDirection: 'left-to-right',
                                itemWidth: 80,
                                itemHeight: 20,
                                itemOpacity: 0.75,
                                symbolSize: 12,
                                symbolShape: 'circle',
                                symbolBorderColor: 'rgba(0, 0, 0, .5)',
                                effects: [
                                    {
                                        on: 'hover',
                                        style: {
                                            itemBackground: 'rgba(0, 0, 0, .03)',
                                            itemOpacity: 1
                                        }
                                    }
                                ]
                            }
                        ]}
                        tooltip={({ point }) => {
                            return (
                                <div
                                    style={{
                                        background: 'white',
                                        padding: '9px 12px',
                                        border: '1px solid #ccc',
                                        color: 'black'
                                    }}
                                >
                                    <div><strong>{point.serieId}</strong></div>
                                    <div>Date: {point.data.xFormatted}</div>
                                    <div>Value: {currencyFormatter(point.data.y as number)}</div>
                                </div>
                            )
                        }}
                    />
                </div>
            )}
        </div>
    )
}

export default App
