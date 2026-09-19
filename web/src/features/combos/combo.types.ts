import type { ComboProviderOption } from '../../lib/types';

export const MAX_COMBO_TARGETS = 32;

export interface ComboTargetDraft { key: number; providerId: string; model: string }

export interface ComboTargetView {
  target: ComboTargetDraft;
  index: number;
  count: number;
  providers: ComboProviderOption[];
}

export interface ComboTargetActions {
  onProviderChange: (targetKey: number, providerId: string) => void;
  onModelChange: (targetKey: number, model: string) => void;
  onActivateModelPicker: (targetKey: number) => void;
  onDeactivateModelPicker: (targetKey: number) => void;
  onMove: (index: number, direction: -1 | 1) => void;
  onRemove: (targetKey: number) => void;
  onOpenProviders: () => void;
}
