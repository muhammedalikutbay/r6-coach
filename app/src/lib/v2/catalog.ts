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

/** Last four digits of a raw id ("441842102525" -> "…2525"), matching the Rust fallback names. */
const idHint = (id: string) => (id.length > 4 ? `…${id.slice(-4)}` : id);
/** A key that is a raw numeric id belongs to an operator or map the tables do not know yet. */
const isRawId = (key: string) => /^\d+$/.test(key);

/** Unknown keys never crash: a readable fallback ("Unknown map (…2525)"), or the key itself. */
export const operatorName = (key: string) =>
  opByKey.get(normKey(key))?.name ?? (isRawId(key) ? `Unknown operator (${idHint(key)})` : key);
export const mapName = (key: string) =>
  mapByKey.get(normKey(key))?.name ?? (isRawId(key) ? `Unknown map (${idHint(key)})` : key);
