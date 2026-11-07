// Global filter store shared by every v2 view. Persisted in localStorage.
import { DEFAULT_FILTER, type Filter } from './types';

const KEY = 'r6k.filter.v2';
const RANGES = ['last1', 'last5', 'last20', 'season', 'all'];
const MODES = ['all', 'ranked', 'unranked', 'quick'];
const SIDES = ['both', 'attack', 'defense'];

const fresh = (): Filter => ({ ...DEFAULT_FILTER, operators: [], maps: [] });
const strings = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);

const dateStr = (v: unknown): string | null => (typeof v === 'string' && /^d{4}-d{2}-d{2}$/.test(v) ? v : null);
const hour = (v: unknown): number | null => (typeof v === 'number' && Number.isInteger(v) && v >= 0 && v <= 23 ? v : null);

function load(): Filter {
  const f = fresh();
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return f;
    const p = JSON.parse(raw);
    if (RANGES.includes(p.range)) f.range = p.range;
    if (MODES.includes(p.mode)) f.mode = p.mode;
    if (SIDES.includes(p.side)) f.side = p.side;
    f.operators = strings(p.operators);
    f.maps = strings(p.maps);
    f.dateFrom = dateStr(p.dateFrom);
    f.dateTo = dateStr(p.dateTo);
    f.hourFrom = hour(p.hourFrom);
    f.hourTo = hour(p.hourTo);
  } catch (e) {
    console.warn('filter: could not read saved filter', e);
  }
  return f;
}

class FilterStore {
  value = $state<Filter>(load());

  private save() {
    try {
      localStorage.setItem(KEY, JSON.stringify(this.value));
    } catch (e) {
      console.warn('filter: could not save filter', e);
    }
  }

  set(patch: Partial<Filter>) {
    this.value = { ...this.value, ...patch };
    this.save();
  }

  toggleOperator(key: string) {
    const ops = this.value.operators;
    this.set({ operators: ops.includes(key) ? ops.filter((k) => k !== key) : [...ops, key] });
  }

  toggleMap(key: string) {
    const maps = this.value.maps;
    this.set({ maps: maps.includes(key) ? maps.filter((k) => k !== key) : [...maps, key] });
  }

  /** True when a date range or hour window is set. */
  get dateActive() {
    const v = this.value;
    return v.dateFrom !== null || v.dateTo !== null || v.hourFrom !== null || v.hourTo !== null;
  }

  /** True when any filter differs from the default. */
  get active() {
    const v = this.value;
    return v.range !== DEFAULT_FILTER.range || v.mode !== 'all' || v.side !== 'both' || v.operators.length > 0 || v.maps.length > 0 || this.dateActive;
  }

  clear() {
    this.value = fresh();
    this.save();
  }
}

export const filterStore = new FilterStore();
