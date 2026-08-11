# Visual design system

Status: **Approved direction; implementation in progress**

## Product feeling

Swarm Next is a capable developer control room presented as a quiet botanical
apiary studio. It should feel soft, natural, playful, and emotionally warm
without resembling a children's product or sacrificing information density.

The identity is expressed through atmosphere and purposeful character moments,
not continuous decoration. Operational content always wins.

## Typography

- Atkinson Hyperlegible Next is the interface family.
- Use regular weight for reading and medium weight for hierarchy.
- Atkinson Hyperlegible Mono is used for identifiers, paths, and metadata.
- Terminal content remains a dedicated, configurable monospace surface.
- Serif display type is excluded because Swarm is text-heavy and scanning speed
  matters more than editorial contrast.

## Color and themes

Light mode uses warm ivory, pale oat, soft sage, charcoal olive, honey, dusty
rose, and restrained lavender. Dark mode is a true botanical adaptation using
forest charcoal, ink blue, warm ivory, sage, muted honey, plum, and rose; it is
not a simple inversion.

State colors always pair color with text or shape:

- honey: active work;
- sage: ready and healthy;
- dusty rose: review;
- muted lavender: blocked;
- neutral olive: draft and completed.

The terminal interior stays near-black and contains no decorative artwork while
output is present. Its ANSI palette is a terminal-specific extension of the
same system:

- sage green: additions, success, and healthy output;
- dusty rose: deletions, errors, and destructive output;
- honey: warnings, active prompts, and emphasized values;
- muted blue and cyan: links, paths, commands, and informational output;
- lavender: metadata, secondary highlights, and special values;
- warm ivory and graduated olive neutrals: primary text, subdued output, and
  comments.

Readable normal and bright ANSI variants remain distinct and meet WCAG AA
contrast against the near-black terminal background. Selection uses translucent
sage,
the cursor uses honey, and application theme changes update the mounted
terminal without recreating its durable session.

## Character system

Characters use simple reusable SVG geometry, flat fills, dark olive outlines,
and expressions that survive at 40-64 pixels.

- Worker: female-coded, slim tapered abdomen, graceful limbs and wings, subtle
  lashes, warm competence.
- Queen: female-coded calm authority, longer wings and abdomen, three
  asymmetric sage leaves and one muted-plum bud; no cartoon crown.
- Drone: male-coded, aerodynamic and slightly sturdier, stronger brows,
  straighter antennae, no lashes; never bulky.

Expressions include available, focused, thinking, blocked, complete, and
sleeping. Emotion must be readable from eyes, brows, mouth, antennae, and pose;
props only reinforce it.

## Illustration placement

- Full characters belong in authentication, onboarding, meaningful empty
  states, and completion moments.
- Worker lists use compact expression portraits; only the selected worker gets
  a strong surrounding ring.
- Queen and Drone art is operational only when those roles exist in product
  state; decoration must not imply nonexistent behavior.
- Botanical marks are reduced around dense work. Use tiny leaf markers,
  flower-shaped status marks, and at most one restrained sprig in genuine empty
  space.

## Density and motion

The shell may breathe while task rows, worker lists, and terminal controls stay
efficient. Final spacing is tuned against populated real screens rather than
concept art. Motion is brief, meaningful, and disabled by reduced-motion
preferences; no looping mascot animation.

## Accessibility

- Text and controls meet WCAG AA contrast in both themes.
- Focus indication is never color-only.
- Theme follows the saved explicit choice, otherwise the system preference.
- Decorative illustrations are hidden from assistive technology; meaningful
  role portraits receive concise labels.
- Character presence never replaces state text.
