import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'

export type TrafficPoint = {
  time: string
  requests?: number
  successful?: number
  cost?: number
  tokens?: number
  latency?: number
}

export default function TrafficChart({
  data,
  metric = 'requests',
  showSuccessful = false,
  gradientId,
  interval,
  margin,
}: {
  data: TrafficPoint[]
  metric?: 'requests' | 'cost' | 'tokens' | 'latency'
  showSuccessful?: boolean
  gradientId: string
  interval?: number
  margin: { top: number; right: number; left: number; bottom?: number }
}) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <AreaChart data={data} margin={margin}>
        <defs>
          <linearGradient id={gradientId} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="var(--chart-accent)" stopOpacity={0.22} />
            <stop offset="100%" stopColor="var(--chart-accent)" stopOpacity={0} />
          </linearGradient>
        </defs>
        <CartesianGrid vertical={false} stroke="var(--chart-grid)" />
        <XAxis
          dataKey="time"
          axisLine={false}
          tickLine={false}
          tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
          interval={interval}
          dy={interval === undefined ? undefined : 10}
        />
        <YAxis
          axisLine={false}
          tickLine={false}
          tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
          allowDecimals={metric === 'requests' || metric === 'tokens' ? false : undefined}
          width={metric === 'tokens' ? 60 : 44}
        />
        <Tooltip
          contentStyle={{
            background: 'var(--panel-solid)',
            border: '1px solid var(--border-strong)',
            borderRadius: 10,
            color: 'var(--text)',
            fontSize: 12,
          }}
        />
        <Area
          type="monotone"
          dataKey={metric}
          stroke="var(--chart-accent)"
          strokeWidth={2}
          fill={`url(#${gradientId})`}
        />
        {showSuccessful && (
          <Area
            type="monotone"
            dataKey="successful"
            stroke="var(--chart-series-2)"
            strokeDasharray="4 4"
            strokeWidth={1.75}
            fill="transparent"
          />
        )}
      </AreaChart>
    </ResponsiveContainer>
  )
}
