import type { ProviderCapabilities, ProviderKind } from "../api";

const choices = [
  { provider: "gemini", label: "Gemini" },
  { provider: "grok", label: "Grok" },
  { provider: "opencode", label: "OpenCode" },
] as const;

export function isExperimentalProvider(provider: ProviderKind): provider is "gemini" | "grok" | "opencode" {
  return choices.some(choice => choice.provider === provider);
}

export function ExperimentalProviderOptions({ selected, current, enabled, providers }: {
  selected: ProviderKind;
  current?: ProviderKind;
  enabled: boolean;
  providers: ProviderCapabilities;
}) {
  return <>{choices.filter(choice => enabled || choice.provider === selected || choice.provider === current).map(choice => {
    const available = providers.experimental?.[choice.provider];
    const existing = choice.provider === current;
    return <option key={choice.provider} value={choice.provider}
      disabled={!existing && (!enabled || available !== true)}>
      {choice.label} · {existing ? "existing experimental provider" : `experimental${available === true ? "" : available === false ? " · unavailable" : " · availability unknown"}`}
    </option>;
  })}</>;
}

export function ExperimentalProviderControl({ enabled, onChange }: { enabled: boolean; onChange: (enabled: boolean) => void }) {
  return <div className="field-stack">
    <label><input type="checkbox" checked={enabled} onChange={event => onChange(event.target.checked)} /> Allow experimental providers for this change</label>
    {enabled && <small>Experimental providers are bare interactive terminals: Swarm tools and automatic conversation recovery are not supported. They are excluded from Night Watch. You can use provider-native commands manually.</small>}
  </div>;
}
