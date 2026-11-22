import type { Snapshot } from "./types";

export type MeetingSpeechPreferences = (
  | { mode: "local" }
  | { mode: "api"; modelId: string }
) & { inputDeviceId?: string; localSpeakerDiarization?: boolean };

export function eligibleMeetingSpeechModels({ models, providers }: Snapshot) {
  const enabledProviders = new Set(
    providers
      .filter((provider) => provider.enabled && provider.protocol !== "anthropic-messages")
      .map((provider) => provider.id),
  );
  return models.filter(
    (model) => model.enabled && model.capability === "speech" && enabledProviders.has(model.providerId),
  );
}
