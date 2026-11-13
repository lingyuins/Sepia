import { Blocks } from 'lucide-react'
import { useTranslation } from '../workspace'
import { SectionHeading } from '../components/UI'

export function PluginsView() {
  const t = useTranslation()
  return <>
    <SectionHeading eyebrow={t('pluginEyebrow')} title={t('pluginsTitle')} description={t('pluginsSubtitle')} />
    <div className="plugin-banner glass-panel"><div className="plugin-banner-icon"><Blocks size={26} /></div><div><h3>{t('pluginsUnavailable')}</h3><p>{t('pluginBannerSubtitle')}</p></div></div>
  </>
}
