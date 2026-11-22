import {
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from 'recharts'

type Category = { name: string; value: number; color: string }

const tooltipStyle = {
  background: 'var(--panel-solid)',
  border: '1px solid var(--border-strong)',
  borderRadius: 10,
  color: 'var(--text)',
  fontSize: 12,
}

export function ProviderPie({ data }: { data: Category[] }) {
  return (
    <ResponsiveContainer width="45%" height={136}>
      <PieChart>
        <Pie
          data={data}
          dataKey="value"
          innerRadius={38}
          outerRadius={56}
          paddingAngle={3}
          stroke="none"
        >
          {data.map((entry) => <Cell key={entry.name} fill={entry.color} />)}
        </Pie>
        <Tooltip contentStyle={tooltipStyle} />
      </PieChart>
    </ResponsiveContainer>
  )
}

export function CategoryBars({
  data,
  labelKey = 'name',
  valueKey = 'requests',
  labelWidth = 105,
  barSize = 12,
  margin = { left: 12, right: 24, top: 8, bottom: 8 },
}: {
  data: Record<string, string | number>[]
  labelKey?: string
  valueKey?: string
  labelWidth?: number
  barSize?: number
  margin?: { left: number; right: number; top?: number; bottom?: number }
}) {
  return (
    <ResponsiveContainer width="100%" height="100%">
      <BarChart data={data} layout="vertical" margin={margin}>
        <CartesianGrid horizontal={false} stroke="var(--chart-grid)" />
        <XAxis
          type="number"
          axisLine={false}
          tickLine={false}
          tick={{ fill: 'var(--chart-text)', fontSize: 11 }}
        />
        <YAxis
          type="category"
          dataKey={labelKey}
          axisLine={false}
          tickLine={false}
          tick={{ fill: 'var(--text-secondary)', fontSize: 11 }}
          width={labelWidth}
        />
        <Tooltip contentStyle={tooltipStyle} cursor={{ fill: 'var(--hover)' }} />
        <Bar dataKey={valueKey} fill="var(--chart-accent)" radius={[0, 4, 4, 0]} barSize={barSize}>
          {data.map((entry, index) => (
            <Cell key={`${entry[labelKey]}-${index}`} fill={String(entry.color || 'var(--chart-accent)')} />
          ))}
        </Bar>
      </BarChart>
    </ResponsiveContainer>
  )
}
