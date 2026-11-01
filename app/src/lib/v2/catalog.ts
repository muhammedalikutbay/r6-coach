// Front-end copy of data/operators.json and data/maps.json (images are resolved by key in OperatorBadge / MapThumb).
import ops from './data/operators.json';
import maps from './data/maps.json';

export interface OperatorEntry { key: string; name: string; side: 'attacker' | 'defender' }
export interface MapEntry { key: string; name: string }

export const OPERATORS: OperatorEntry[] = (ops.operators as OperatorEntry[])
  .map((o) => ({ key: o.key, name: o.name, side: o.side }))
  .sort((a, b) => a.name.localeCompare(b.name));
/** Single asset/lookup key: lowercase alphanumerics only ("calypso-casino" -> "calypsocasino"). */
export const normKey = (s: string) => s.toLowerCase().replace(/[^a-z0-9]/g, '');

export const MAPS: MapEntry[] = [
  ...new Map((maps.maps as MapEntry[]).map((m) => [normKey(m.key), { key: normKey(m.key), name: m.name }])).values()
].sort((a, b) => a.name.localeCompare(b.name));

const opByKey = new Map(OPERATORS.map((o) => [o.key, o]));
const mapByKey = new Map(MAPS.map((m) => [m.key, m]));

/** Unknown keys never crash: fall back to the raw key. */
export const operatorName = (key: string) => opByKey.get(normKey(key))?.name ?? key;
export const mapName = (key: string) => mapByKey.get(normKey(key))?.name ?? key;
