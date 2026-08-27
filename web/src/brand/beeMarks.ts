/**
 * The mark a worker wears, so one repository can be told from another.
 *
 * WHY NOT COLOUR. The first design varied the bee's hue. The operator: "coloured
 * bees feels unnatural in a certain way too" — and they are right, a plum bee
 * reads as a tinted shape rather than as a bee. Colour lives on the accessory
 * instead, where a red cap is perfectly natural.
 *
 * WHY NOT SILHOUETTE. The second design varied the body by species. At 34px a
 * rounder abdomen is invisible, and up close they stop being the same character
 * and become different animals.
 *
 * WHY NOT THE FACE. `expression` already owns the eyes, brows and mouth, and it
 * is driven from live state. A worker whose identity was "the sleeping one"
 * would look asleep while it worked. Every mark here sits ABOVE the eye line,
 * OUTSIDE the head, or BELOW the collar, so identity and state never contradict
 * each other — a bee can be the one with the glasses AND be blocked.
 */
export type BeeMark =
  | "plain"
  | "spectacles"
  | "cap"
  | "beanie"
  | "hardhat"
  | "party"
  | "headphones"
  | "flower"
  | "leaf"
  | "bow"
  | "goggles"
  | "bandana"
  | "wreath"
  | "scarf"
  | "hairband"
  | "monocle"
  | "bun"
  | "buns"
  | "pigtails"
  | "ponytail"
  | "braids"
  | "fringe"
  | "curls";

/**
 * Every mark, in a fixed order.
 *
 * The order is part of the contract: `markFor` indexes into it, so reordering
 * this list silently reassigns every worker's bee. Add to the END.
 */
export const BEE_MARKS: readonly BeeMark[] = [
  "plain",
  "spectacles",
  "cap",
  "beanie",
  "hardhat",
  "party",
  "headphones",
  "flower",
  "leaf",
  "bow",
  "goggles",
  "bandana",
  "wreath",
  "scarf",
  "hairband",
  "monocle",
  "bun",
  "buns",
  "pigtails",
  "ponytail",
  "braids",
  "fringe",
  "curls",
];

/** What each one is called where an operator chooses it. */
export const BEE_MARK_LABELS: Record<BeeMark, string> = {
  plain: "Plain",
  spectacles: "Spectacles",
  cap: "Cap",
  beanie: "Beanie",
  hardhat: "Hard hat",
  party: "Party hat",
  headphones: "Headphones",
  flower: "Flower",
  leaf: "Leaf",
  bow: "Bow",
  goggles: "Goggles",
  bandana: "Bandana",
  wreath: "Wreath",
  scarf: "Scarf",
  hairband: "Hairband",
  monocle: "Monocle",
  bun: "Top bun",
  buns: "Twin buns",
  pigtails: "Pigtails",
  ponytail: "Ponytail",
  braids: "Braids",
  fringe: "Fringe",
  curls: "Curls",
};

/**
 * The mark a worker wears when nobody has chosen one.
 *
 * ASSIGNED AT RANDOM, BUT NOT RANDOMLY DECIDED. It is derived from the worker's
 * identity rather than drawn from a random source, so the same worker gets the
 * same bee on every surface, in every browser, after every restart. A genuinely
 * random pick would give the roster a different face on each render, which is
 * worse than everyone looking alike.
 *
 * FNV-1a: small, stable, and the point is spreading over the set rather than
 * resisting anybody. Nothing here is a secret.
 */
export function markFor(identity: string): BeeMark {
  let hash = 0x811c_9dc5;
  for (let index = 0; index < identity.length; index += 1) {
    hash ^= identity.charCodeAt(index);
    // >>> 0 keeps it unsigned; JavaScript's bitwise operators are signed 32-bit
    // and a negative hash would index off the front of the list.
    hash = Math.imul(hash, 0x0100_0193) >>> 0;
  }
  return BEE_MARKS[hash % BEE_MARKS.length];
}

/**
 * The mark to draw: the operator's choice if they made one, else the derived one.
 *
 * An unrecognised stored value falls through to the derived mark rather than
 * rendering nothing. A worker whose bee came back from an older build, or from a
 * mark that has since been removed, still gets a bee.
 */
export function resolveMark(identity: string, chosen?: string | null): BeeMark {
  if (chosen && (BEE_MARKS as readonly string[]).includes(chosen)) {
    return chosen as BeeMark;
  }
  return markFor(identity);
}
