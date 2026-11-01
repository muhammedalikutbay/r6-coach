// Global filter store shared by every v2 view (contract only; the FilterBar task adds UI and persistence).
import { DEFAULT_FILTER, type Filter } from './types';

class FilterStore {
  value = $state<Filter>({ ...DEFAULT_FILTER, operators: [], maps: [] });

  set(patch: Partial<Filter>) {
    this.value = { ...this.value, ...patch };
  }

  toggleOperator(key: string) {
    const ops = this.value.operators;
    this.set({ operators: ops.includes(key) ? ops.filter((k) => k !== key) : [...ops, key] });
  }

  toggleMap(key: string) {
    const maps = this.value.maps;
    this.set({ maps: maps.includes(key) ? maps.filter((k) => k !== key) : [...maps, key] });
  }

  clear() {
    this.value = { ...DEFAULT_FILTER, operators: [], maps: [] };
  }
}

export const filterStore = new FilterStore();
