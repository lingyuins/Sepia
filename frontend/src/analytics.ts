export function chartTimeLabel(value: string, period: string, language: string, timeZone?: string) {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat(language, period === '24 hours'
    ? { hour: '2-digit', minute: '2-digit', hour12: false, timeZone }
    : { month: 'numeric', day: 'numeric', timeZone },
  ).format(date)
}
