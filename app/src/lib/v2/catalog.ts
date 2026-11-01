// Front-end copy of data/operators.json and data/maps.json (images are resolved by key in OperatorBadge / MapThumb).
import ops from './data/operators.json';
import maps from './data/maps.json';

export interface OperatorEntry { key: string; name: string; side: 'attacker' | 'defender' }
export interface MapEntry { key: string; name: string }

export const OPERATORS: OperatorEntry[] = (ops.operators as OperatorEntry[])
  .map((o) => ({ key: o.key, name: o.name, side: o.side }))
  .sort((a, b) => a.name.localeCompare(b.name));
export const MAPS: MapEntry[] = (maps.maps as MapEntry[])
  .map((m) => ({ key: m.key, name: m.name }))
  .sort((a, b) => a.name.localeCompare(b.name));

const opByKey = new Map(OPERATORS.map((o) => [o.key, o]));
const mapByKey = new Map(MAPS.map((m) => [m.key, m]));

/** Unknown keys never crash: fall back to the raw key. */
export const operatorName = (key: string) => opByKey.get(key)?.name ?? key;
export const mapName = (key: string) => mapByKey.get(key)?.name ?? key;
