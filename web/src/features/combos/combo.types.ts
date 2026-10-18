import type { ComboProviderOption, GatewayCombo } from '../../lib/types';

/** Mirrors `MAX_ROUTE_TARGETS` in src/config/limits.rs (TypeScript cannot import it). */
export const MAX_COMBO_TARGETS = 32;

/** Whether a target row names a provider or nests another combo. */
export type ComboTargetKind = 'provider' | 'combo';

export interface ComboTargetDraft {
  key: number;
  kind: ComboTargetKind;
  providerId: string;
  model: string;
  comboId: string;
}

export interface ComboTargetView {
  target: ComboTargetDraft;
  index: number;
  count: number;
  providers: ComboProviderOption[];
  /** Combos this row may nest: the loaded combos minus the ones that would close a reference cycle. */
  combos: GatewayCombo[];
}

export interface ComboTargetActions {
  onKindChange: (targetKey: number, kind: ComboTargetKind) => void;
  onProviderChange: (targetKey: number, providerId: string) => void;
  onModelChange: (targetKey: number, model: string) => void;
  onComboChange: (targetKey: number, comboId: string) => void;
  onActivateModelPicker: (targetKey: number) => void;
  onDeactivateModelPicker: (targetKey: number) => void;
  onMove: (index: number, direction: -1 | 1) => void;
  onRemove: (targetKey: number) => void;
  onOpenProviders: () => void;
}
