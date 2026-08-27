import { type BeeMark } from "./beeMarks";

export type BeeRole = "worker" | "queen" | "drone";
export type BeeExpression = "available" | "focused" | "thinking" | "blocked" | "complete" | "sleeping";

type Props = {
  role?: BeeRole;
  expression?: BeeExpression;
  className?: string;
  label?: string;
  /**
   * What this bee wears, so one worker can be told from another.
   *
   * Absent means plain. Nothing it draws enters the eye or mouth band, so it
   * cannot change what `expression` is reporting.
   */
  mark?: BeeMark;
};

export default function BeeMascot({ role = "worker", expression = "available", className = "", label, mark = "plain" }: Props) {
  const female = role !== "drone";
  const sleeping = expression === "sleeping";
  const queen = role === "queen";
  const drone = role === "drone";
  return (
    <svg
      className={`bee-mascot bee-${role} bee-${expression} bee-mark-${mark} ${className}`.trim()}
      viewBox="0 0 120 120"
      role={label ? "img" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
    >
      {sleeping && <path className="bee-leaf" d="M18 96C43 77 79 75 108 91 88 111 48 116 18 96Z" />}
      <g className="bee-wings">
        <ellipse cx={queen ? 34 : 38} cy="47" rx={queen ? 23 : 19} ry={queen ? 12 : 11} transform="rotate(-38 38 47)" />
        <ellipse cx={queen ? 85 : 80} cy="45" rx={queen ? 23 : 18} ry={queen ? 12 : 10} transform="rotate(35 80 45)" />
        <path d="M25 46 45 48M72 47l20-4" />
      </g>
      <path className="bee-abdomen" d={drone ? "M35 68C38 52 79 50 91 72 95 84 87 104 66 109 48 108 31 89 35 68Z" : queen ? "M40 66C45 51 76 50 83 67 89 86 76 110 61 115 45 106 34 84 40 66Z" : "M39 66C43 52 76 51 83 68 88 84 77 106 61 111 45 104 34 83 39 66Z"} />
      <path className="bee-stripe" d="M38 75c14 8 33 9 47 1M40 91c12 7 27 8 40 2" />
      <path className="bee-leg" d={sleeping ? "M41 91c-8 5-12 8-17 6M77 94c7 3 12 4 18 2" : "M46 96c-4 9-7 13-12 16M74 97c3 8 7 12 12 15"} />
      <BeeHair mark={mark} />
      <circle className="bee-head" cx="61" cy="43" r={drone ? 27 : 26} />
      <path className="bee-collar" d="M38 62c5-8 11-9 16-4 5-6 11-6 16 0 6-5 12-2 15 5-8 8-37 9-47-1Z" />
      <g className="bee-antennae">
        <path d={drone ? "M48 21 42 5M72 20 79 5" : "M49 20C47 12 42 8 38 5M72 20c3-8 8-12 13-14"} />
        <circle cx={drone ? 42 : 38} cy="5" r="3" />
        <circle cx={drone ? 79 : 85} cy={drone ? 5 : 6} r="3" />
      </g>
      {queen && <g className="bee-diadem"><path d="m47 19 2-11 8 8 4-13 5 13 9-8-1 12" /><circle cx="61" cy="7" r="2.5" /></g>}
      <BeeHairFront mark={mark} />
      <BeeFace expression={expression} female={female} />
      <BeeWorn mark={mark} />
      {!sleeping && <path className="bee-arm" d={expression === "thinking" ? "M77 69c10-4 11-11 7-17" : expression === "blocked" ? "M78 70c10-4 14-9 17-16" : "M43 70c-8 4-12 8-14 14"} />}
      {expression === "complete" && <g className="bee-complete-mark"><circle cx="94" cy="82" r="16" /><path d="m86 82 6 6 11-14" /></g>}
      {expression === "blocked" && <g className="bee-blocked-mark"><path d="M91 79h19M100.5 69v20" /></g>}
      {sleeping && <g className="bee-sleep-mark"><path d="m88 35 10-8H88l10-8" /><path d="m99 48 7-6h-7l7-6" /></g>}
    </svg>
  );
}

function BeeFace({ expression, female }: { expression: BeeExpression; female: boolean }) {
  if (expression === "sleeping" || expression === "complete") {
    return <g className="bee-face"><path className="bee-eye-line" d="M48 42q5 5 10 0M66 42q5 5 10 0" />{female && <path className="bee-lashes" d="m48 41-3-2m31 2 3-2" />}<path className="bee-mouth" d="M57 52q5 5 10 0" /></g>;
  }
  const focused = expression === "focused";
  const blocked = expression === "blocked";
  return <g className="bee-face">
    <ellipse className="bee-eye" cx="52" cy="42" rx={focused ? 3.4 : 4} ry={focused ? 4.5 : 5} />
    <ellipse className="bee-eye" cx="71" cy="42" rx={focused ? 3.4 : 4} ry={focused ? 4.5 : 5} />
    <circle className="bee-eye-glint" cx="53" cy="40" r="1" /><circle className="bee-eye-glint" cx="72" cy="40" r="1" />
    {female && <path className="bee-lashes" d="m48 37-3-2m3 5-4-1m31-2 3-2m-3 5 4-1" />}
    {focused && <path className="bee-brow" d="m47 34 9 1m11 0 9-1" />}
    {blocked && <path className="bee-brow" d="m47 34 8-2m12 0 8 3" />}
    <path className="bee-mouth" d={focused ? "M58 53h7" : blocked ? "M57 55q5-5 10 0" : expression === "thinking" ? "M58 53q5 3 9-1" : "M57 52q5 6 11 0"} />
  </g>;
}

/**
 * Hair, drawn BEFORE the head so the head overlaps it.
 *
 * That overlap is the whole trick: the same shape drawn after the head reads as
 * a hat sitting on top, and drawn before it reads as hair growing out.
 *
 * Every coordinate here was checked against the avatar's real clip — a 38px bee
 * inside a 34px circle shifted down 4px, which leaves a visible disc of centre
 * (60, 47.4) radius 53.7 in these units. Seven marks were redrawn after that
 * audit; the party hat's tip had been at y -12, outside the viewBox entirely.
 */
function BeeHair({ mark }: { mark: BeeMark }) {
  switch (mark) {
    case "bun":
      return <g className="bee-hair"><circle cx="61" cy="12" r="12" /></g>;
    case "buns":
      return <g className="bee-hair"><circle cx="31" cy="26" r="11" /><circle cx="91" cy="26" r="11" /></g>;
    case "pigtails":
      return <g className="bee-hair"><path d="M30 34c-11 2-17 12-15 23 2 10 11 15 20 12 6-2 8-8 6-14Z" /><path d="M92 34c11 2 17 12 15 23-2 10-11 15-20 12-6-2-8-8-6-14Z" /></g>;
    case "ponytail":
      return <g className="bee-hair"><path d="M84 26c11 1 20 11 21 24 1 12-6 21-14 22-5 0-7-4-6-9 2-11 1-23-6-32Z" /></g>;
    case "braids":
      return <g className="bee-hair"><path d="M31 36c-8 6-10 18-6 30 3 9 11 12 17 8 5-4 5-10 2-15Z" /><path d="M91 36c8 6 10 18 6 30-3 9-11 12-17 8-5-4-5-10-2-15Z" /></g>;
    case "curls":
      return <g className="bee-hair"><circle cx="38" cy="22" r="9" /><circle cx="61" cy="13" r="10" /><circle cx="84" cy="22" r="9" /></g>;
    default:
      return null;
  }
}

/** The parts of a hairstyle that belong in front of the head. */
function BeeHairFront({ mark }: { mark: BeeMark }) {
  switch (mark) {
    case "bun":
      return <g className="bee-hair"><path className="bee-hair-line" d="M50 16c7-5 15-5 22 0" /></g>;
    case "ponytail":
      return <g className="bee-hair"><path className="bee-hair-line" d="M82 28c7 1 12 6 15 12" /></g>;
    case "braids":
      return <g className="bee-hair"><path className="bee-hair-line" d="M27 46h11M28 56h11M84 46h11M83 56h11" /></g>;
    case "fringe":
      return <g className="bee-hair"><path d="M36 36c2-14 12-23 25-23s23 9 25 23c-6-9-14-7-17 0-4-9-13-9-17 0-3-7-11-9-16 0Z" /></g>;
    default:
      return null;
  }
}

/** Worn marks, drawn over the face but never across it. */
function BeeWorn({ mark }: { mark: BeeMark }) {
  switch (mark) {
    case "spectacles":
      return <g className="bee-kit"><circle cx="52" cy="42" r="9" /><circle cx="71" cy="42" r="9" /><path d="M61 42h1M43 40l-6-3M80 40l6-3" /></g>;
    case "monocle":
      return <g className="bee-kit"><circle cx="71" cy="42" r="10" /><path d="M81 42c5 0 8 4 8 9" /></g>;
    case "cap":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M36 33c2-16 13-24 25-24s23 8 25 24c-16-6-34-6-50 0Z" /><path className="bee-kit-fill" d="M84 30c11 0 18 3 20 7-8 5-17 5-24 2Z" /></g>;
    case "beanie":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M37 30c1-15 12-23 24-23s23 8 24 23c-16-6-32-6-48 0Z" /><path className="bee-kit-fill" d="M35 29c17-6 35-6 52 0v8c-17-6-35-6-52 0Z" /><circle className="bee-kit-heart" cx="61" cy="10" r="6" /></g>;
    case "hardhat":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M40 30c1-15 10-22 21-22s20 7 21 22c-14-5-28-5-42 0Z" /><path className="bee-kit-fill" d="M31 31c19-7 41-7 60 0 0 5-3 7-8 7H39c-5 0-8-2-8-7Z" /><path d="M61 8v22" /></g>;
    case "party":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M61 10 76 32c-10-4-20-4-30 0Z" /><circle className="bee-kit-heart" cx="61" cy="8" r="5" /><path d="M53 24h16" /></g>;
    case "headphones":
      return <g className="bee-kit"><path d="M31 46C31 24 44 11 61 11s30 13 30 35" /><rect className="bee-kit-fill" x="24" y="38" width="15" height="20" rx="7" /><rect className="bee-kit-fill" x="83" y="38" width="15" height="20" rx="7" /></g>;
    case "flower":
      return <g className="bee-kit"><g className="bee-kit-fill"><circle cx="84" cy="24" r="6" /><circle cx="96" cy="21" r="6" /><circle cx="90" cy="11" r="6" /><circle cx="80" cy="13" r="6" /><circle cx="98" cy="31" r="6" /></g><circle className="bee-kit-heart" cx="89" cy="21" r="4.5" /></g>;
    case "leaf":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M80 30c2-11 11-17 20-16 2 9-6 18-16 20-3 0-4-2-4-4Z" /><path d="M84 31c5-4 10-7 15-8" /></g>;
    case "bow":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M83 24c-6-6-13-5-13 1s6 9 13 4Zm5 0c6-6 13-5 13 1s-6 9-13 4Z" /><circle className="bee-kit-heart" cx="85.5" cy="25" r="4" /></g>;
    case "goggles":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M36 26c15-8 35-8 50 0v6c-16-7-34-7-50 0Z" /><circle cx="51" cy="24" r="8" /><circle cx="71" cy="23" r="8" /><path d="M59 24h4" /></g>;
    case "bandana":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M36 28c16-8 34-8 50 0-1 5-2 7-3 9-15-6-30-6-45 0-1-2-2-4-2-9Z" /><path className="bee-kit-fill" d="M35 30c-6 1-10 4-11 8 5 3 10 3 14 1Z" /></g>;
    case "hairband":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M37 30c15-8 33-8 48 0v7c-15-7-33-7-48 0Z" /><path className="bee-kit-fill" d="M83 30c-6-6-12-5-12 1s7 9 12 3Zm4 0c6-6 12-5 12 1s-7 9-12 3Z" /><circle className="bee-kit-heart" cx="85" cy="32" r="3.5" /></g>;
    case "wreath":
      return <g className="bee-kit"><g className="bee-kit-fill"><circle cx="41" cy="33" r="5" /><circle cx="46" cy="24" r="5" /><circle cx="54" cy="18" r="5" /><circle cx="68" cy="18" r="5" /><circle cx="76" cy="24" r="5" /><circle cx="81" cy="33" r="5" /></g></g>;
    case "scarf":
      return <g className="bee-kit"><path className="bee-kit-fill" d="M36 66c9 10 41 9 50 0 3 5 3 10 0 14-10 9-42 10-51 1-3-4-2-10 1-15Z" /><path className="bee-kit-fill" d="M84 78c4 4 5 9 4 12-3 2-7 0-8-2Z" /></g>;
    default:
      return null;
  }
}
